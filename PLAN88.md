# PLAN88: What Six Audits Found

Status: written 2026-09-13 from six independent read-only audits run in
parallel over the whole codebase. Supersedes nothing. `PLAN77.md` is the record
of what was built; this file is the record of what is **wrong with it**.

Every finding below was verified by its auditor — by reading the code, by
driving a scratch harness, by measuring in real Chrome, or by building a
working proof of concept. Findings the auditors could not confirm are marked
**suspected** and are not ranked. Two auditors independently reached the same
conclusion in two places; both are noted.

Baseline at the time of audit: 1,098 tests passing, e2e 45/45, clippy clean.
**Those numbers did not catch any of the P0 items below.** That is the most
important sentence in this file, and §7 is about why.

---

## The shape of the problem

Six audits, run against a codebase whose gate was green, found:

- **one critical vulnerability** with a working proof of concept, reachable by
  sending someone a file,
- **three ways to corrupt or fabricate a user's document**, two of them
  verified by running,
- **one active regression introduced earlier the same day** that makes
  previously signed repositories unopenable,
- **a 250-function formula engine with zero tests** and six wrong answers found
  in an afternoon,
- **a convergence test family that is true by construction** and proves nothing,
- **a 2.6 second no-op** on a 2,000-row spreadsheet, caused by building a field
  nothing reads.

The common thread is not carelessness. It is that **the tests assert the shape
of the answer rather than the answer**, and the gate grew around them.

---

## P0 — fix before anything else

### P0-1. Stored XSS via LaTeX → arbitrary local file read/write
*Security audit. Verified with two scratch binaries; the final DOM step is
inference, not observation.* **Fix in flight.**

`math-core` 0.8.2 does not escape the argument of `\operatorname`, and
`opendoc-render` writes its output raw (three `out.push_str(&rendered.html)`
sites in `context.rs`). A `Document` carrying the payload **passes
`validate()`**.

Reachability is not self-XSS:
`opendoc-import/src/opendoc_json.rs::import_opendoc_inline_equation` takes
`opendocEquation.source` **verbatim**, so sending someone a `.json` file they
import is code execution. So is one collaborator's equation over the service.

Escalation: `tauri.conf.json` sets `"csp": null` with `withGlobalTauri: true`,
so injected script reaches `window.__TAURI__.core.invoke`, and
`read_file_base64` / `write_file_base64` / `write_file_text` take
**unrestricted paths** — the generated permission says "without any
pre-configured scope". With no CSP, exfiltration needs no Tauri command at all.

Three independent fixes, all small: escape math-core's output (it is not a
trusted producer), set a real CSP, and confine the file commands to handles
minted by the pickers.

### P0-2. Table cells have no column identity
*Collaboration audit. Verified by fuzz + a deterministic 3-operation repro.*
**Fix in flight.**

`TableCell` carries no column reference — a cell belongs to a column only by
its index in `row.cells`. ADR 0013 applied its own argument ("an index means
different things on two replicas, an identity does not") to rows and columns
but not one level down.

Concurrent row-insert plus column-delete makes two identically shaped rows
**lose different cells**, reported only as a generic `table-geometry-repaired`
warning. Every replica agrees on the same wrong table — the ADR 0007 failure
mode, one level down. In **145 of 6,000 seeds (2.4%)** of a realistic 3-actor
table workload it escalates to `merge_operations` returning `Err`.

### P0-3. Derived values are inside the signing payload, and a mismatch bricks the document
*Storage audit. Both halves verified by running.* **Fix in flight.**

`signing_document_from_snapshot` clears several derived fields but **not**
`word_count`, `character_count`, `Cell::display_value` or `spill_source`. And
`read_signatures` **aborts the entire open** when a signature does not match —
there is no `broken` state, though ADR 0003 requires one and
`verify_manifest_blobs` already degrades a bad blob to a warning.

**This is live.** The `visible_text()` change that landed earlier today moved
`word_count`/`character_count`, and therefore the bytes every existing
signature covers. By the second half, that does not show a broken badge — it
makes those repositories **unopenable**.

### P0-4. Opening a service-written repository fabricates demo data into it
*Storage audit. Verified by running.* **Fix in flight.**

`AppDocument::from_core` unconditionally sets `workbook: sample()` — the
"Prototype Sheet" demo. Opening a service repository yields six fabricated
cells with **no warning**; the next save commits them, inside the snapshot a
signature covers. This routes around the invariant `state.rs` states outright
and that `#[cfg(test)]` on `new_sample` was meant to make a compile error.

### P0-5. Untrusted files abort or wedge the app
*Security audit. Three PoCs, all reproducing.* **Fix in flight.**

| Input | Size | Result |
|---|---|---|
| `.docx`, 40,000 nested elements | **2,607 B** | stack overflow — SIGABRT, **uncatchable**, kills the host |
| `.docx` inflating to 1 GiB | 1 MB | **3.1 GB** peak RSS |
| `.xlsx`, one cell at row 65,000 | ~5 KB | **15.5 s**, quadratic → ~66 min at the row maximum |

No depth limit in `walk_blocks`, no `Read::take` in `docx/package.rs`, no row
cap in `spreadsheet/io.rs` — though `google.rs` already has the constant the
XLSX path lacks.

### P0-6. `invalidation_order` is O(n²) to build and nothing reads it
*Spreadsheet audit. Measured.* **Fix in flight.**

`build_dependency_graph` calls `collect_invalidation_order` once per node, each
walking that node's whole transitive dependent set. A **no-op** `evaluate()` on
a 2,000-row running-total column costs **2.6 s**; one cell edit end to end at
1,000 rows costs **~5 s** and hands TypeScript a **7.9 MB** DTO, of which 94%
is this field. It crosses the WASM boundary on every command and is cloned into
every undo checkpoint. No consumer exists anywhere.

### P0-7. Six wrong formula answers
*Spreadsheet audit. All driven.* **Fix in flight.**

`COUNT` over a range containing an error returns the error instead of skipping
it; `MAX`/`MIN` over an empty range return `#N/A` instead of 0; one-argument
`ROUND` errors; `FLOOR` fails on **any** negative number; nested `SUBTOTAL`
double-counts (defeating the function's entire purpose); and `=2^63` displays
as `9223372036854775808` because `trim_number` does `value as i64`, which
saturates in Rust.

### P0-8. Typed spreadsheet input is never parsed
*Spreadsheet audit. Driven.* **Fix in flight.**

`format::parse_input` has **zero call sites in the tree**. `50%`, `$5`,
`1,000` and `2024-01-05` all store strings, so `=B1+1` on a typed date gives
`#VALUE!` and a column of typed percentages sums to zero.

### P0-9. A merge failure is invisible three times over
*Collaboration audit. Verified by reading; reachability established by P0-2.*
**Fix in flight.**

- `DocumentOperationService::apply`/`apply_batch` swallow it (`if let Ok(...)`,
  returning `()`): the operation is not journalled, nothing advances, **no
  error reaches the caller**, and `dispatch_command` returns success. The
  user's gesture silently does nothing.
- `apply_remote_operations` journals envelopes **before** merging, so one
  failure freezes the replica permanently — every later keystroke fails
  identically.
- The undo stack pushes a refused step back, so one spreadsheet edit or image
  upload **wedges Ctrl+Z for the rest of the session**, including for all the
  ordinary typing underneath it.

---

## P1 — wrong output, not yet fixed

### P1-1. `MarkKind::Size` does not change the computed leading
*Rendering audit. Measured against real Chrome.*

`Engine::text_fragment` takes leading from the block's base size and ignores
run size marks; Chrome's unitless `line-height: 1.5` multiplies the span's own
size. An 18pt paragraph is 22px/line in Rust and 36px in Chrome — **reported as
`exact: true`**. One such paragraph among 40 plain ones puts **8 of 41 blocks
outside the content box of the page Rust assigned them**, and the PDF sets
18pt glyphs on a 16.5pt baseline grid.

This is precisely the failure ADR 0014's agreement check exists to detect.

### P1-2. Inline CSS padding is unmodelled width
*Rendering audit. Measured.*

`.mark-code`, `.citation-label` and `.mention` carry 2px/2px/6px of padding the
layout never measures, so ADR 0014's guarantee — a run's width **is** the sum
of its advances — is false for those kinds. Measured to change the line count
on a real paragraph. Note `standalone.rs` already omits the padding, so the
HTML export agrees with the layout and only the app's stylesheet is out of
step.

### P1-3. Applying a mark skips blocks after any non-text block
*Text audit. Verified in real Chrome.* **FIXED (wave 2).** `boundaries` in
`opendoc-app/src/editor.rs::apply_editor_mark` is now `Vec<(block, from, to)>`
carrying the block index, so pass 2 no longer reads pairs back positionally.

`apply_editor_mark` builds `boundaries` only for text blocks, then indexes it
with an `enumerate` counting **every** block. Select three paragraphs with a
page break after the first, click Bold: the first two go bold, the third
silently does not. The document also gains gratuitously split runs.

### P1-4. DOCX tables: import misaligns columns, export drops everything silently
*Storage audit. Verified against a LibreOffice-produced DOCX.*
**ALREADY FIXED when wave 3 arrived** — see "P1-4 was already done" below. The
finding was accurate about the pre-fix state; the fix had landed in untracked
work (`src/docx/table.rs`) that the audit did not see.

Import counts `gridSpan`/`vMerge` and warns "imported unmerged", but does not
insert the covered grid positions — so cells land in **the wrong columns**.
`gridCol` widths are dropped with **no warning at all**. Export writes no
`gridSpan`/`vMerge`, equalises widths, and drops cell background, borders and
vertical alignment — with **zero** warnings, violating ADR 0010. The ODT writer
does all of it correctly, so the model supports it.

### P1-5. `restore_version` desynchronises the snapshot from its own log
*Storage audit. Verified by replaying the manifest chain.*

Restore journals a bare envelope with no typed operation, so the committed
head's snapshot is no longer `merge_operations(genesis, all segments)` —
replay reproduces the pre-restore document. `merge_repository_candidates_inner`
derives merged blocks from exactly that replay, so a divergent candidate across
a restore would resurrect restored-away content.

### P1-6. Crash recovery silently discards a signature
*Storage audit. Verified by running.*

`recover_session` clears signatures and the segment header carries none.
Sign → crash → recover gives `state=unsigned, sigs=0, warnings=[]`. The UI
offers *"still had 0 unsaved changes"*, so the user cannot tell a signature is
at stake either way.

### P1-7. Two runtimes over one recovery store kill crash protection permanently
*Storage audit. Verified end to end.*

A second window is offered the first's **live** journal as a crash. If it
discards, the first's appends fail forever and the cursor is never reset, so it
never re-snapshots. The warning dedupes, then silence. ADR 0005/0008 document
"one session at a time" as a limitation — not as permanent, non-self-healing
failure. The self-healing half is ~3 lines.

### P1-8. Client submits are uncapped and a rejection wedges the session forever
*Collaboration audit.*

`MAX_OPERATIONS_PER_SUBMIT = 512` is enforced only server-side. Neither client
chunks. A replayed disconnection or a large paste builds an oversized submit,
the service rejects it, `blocked` is set — and is cleared only by a fresh
welcome, so reconnecting re-derives the same batch and is rejected again. There
is no automatic way out.

### P1-9. A dropped `Committed` frame leaves the replica silently diverged
*Collaboration audit.*

An `Err` from `apply_remote_operations` sets a notice and returns; nothing
blocks, forces a reconnect or re-requests. The **whole commit is lost** while
the session stays Live and keeps submitting. The native path also advances
`commit_seq` for a commit it did not apply.

### P1-10. The image-pixel signature attests to nothing
*Security audit. Found independently twice — from the code and from its test.*
**FIXED (wave 2).** `sign_image_pixels_blob` now requires the blob bytes to be
present and digests the container, so the signed frame names the bytes it is
about and a transplanted attestation no longer verifies.

`sign_image_pixels_blob` takes width/height/pixels from the **command
arguments** and never decodes the blob; verification recomputes from the same
caller-supplied payload stored beside the signature, so it can never disagree.
A signed attestation can be transplanted onto different image bytes. Its one
test constructs the "different" case field-for-field identical to the first.

### P1-11. The SSRF guard is check-then-connect
*Security audit.* **The code was already fixed; the proof was not.** See
"Fixed, but nothing could have told you" below — the one test that claimed to
establish the pinning asserted `client.is_ok()` and could not fail.

`guard_fetch_url` resolves, inspects, **discards the result**, then reqwest
resolves independently — DNS rebinding defeats it unconditionally. A configured
proxy defeats it too (reqwest is built with `system-proxy`, so the proxy
resolves the name). Gaps in `is_private_address`: no `192.0.0.0/24`, no
`198.18.0.0/15`, no 6to4/Teredo wrapping a private v4, and `::127.0.0.1` is not
caught by `to_ipv4_mapped`. Both functions are pure and have **zero tests**.
Mitigating: the only caller is a dialog the user must paste a URL into.

---

## P2 — re-verified against the tree, 2026-09-13

The original P2 list had gone badly stale. A read-only sweep re-checked every
bullet against the working tree **including untracked files**, which is where
four of the decisive fixes were hiding. Result: **14 fixed, 5 partly fixed,
2 still open, 0 undetermined.** All three bullets marked "*Fix in flight*" had
in fact landed. One item landed *during* the audit.

### Fixed (14)

| # | Item | Where it is now |
|---|---|---|
| 1 | Backspace can never delete an image | `editor.ts:392` uses `sideOfBlock` instead of a hardcoded 0; `editor.rs:568` deletes an atomic block a range covers. **Residue: still no menu action to delete an image.** |
| 2 | Undo refusals reported as "Nothing to undo" | `state.rs:499` raises two distinct `Conflict`s; `actions.ts:541` says "Nothing to undo" only for that exact payload |
| 3 | `rebase_selection` written and never called | Called at `collaboration.rs:260`; real selections passed from `opendoc-wasm/src/collab.rs` and `src-tauri/src/collab.rs` |
| 4 | Spreadsheet merges: covered cells writable/navigable/counted | All four sub-claims fixed. The summary test's own docstring records that it was rewritten to build the state directly, because going through `set_cell_in_sheet` "would pass whether or not the summary skips covered cells" — §7 caught in the act |
| 5 | `strict` cell validation enforces nothing | `workbook.rs:305` → `refuse_hidden_or_invalid_cell_write`; logic in the **untracked** `validation.rs` |
| 6 | XLSX import invisible to replay | Journals `ReplaceWorkbook { workbook }`; the test deliberately diverges the pre-import state so "happened" and "did not" are distinguishable |
| 7 | Spreadsheet envelope ordering lexicographic (SH-9) | `spreadsheet_replay_order` returns a typed `(String, u64)`; test asserts the tenth edit wins |
| 9 | Selection across a table boundary; Enter in a cell | `delete_across_containers`, documented as replacing "the old answer, which was to do nothing at all and report success"; Enter fixed in `blocks.rs` |
| 10 | Tab is a focus trap | `shell.ts:230` — list → indent, table → next cell, otherwise let through; Escape blurs |
| 11 | RTL finished in Rust, unreachable from the UI | `menus.ts:136` → `actions.ts:96` → `toolbar.ts:286` |
| 14 | PDF drops colour/underline/links, footnote `0`, no warnings | All drawn; `pdf-footnotes-after-the-body` named |
| 15 | Ordered-list numbering implemented twice | Deduplicated into `opendoc-layout::lists::ListNumbering`. **Landed during the audit** — see the caveat below |
| 19 | Two browser tabs clobber each other's storage | Web Lock; wave 3 |
| 21 | `storage_ready()`'s report is discarded | Persistent banner; wave 3 |

### Partly fixed (5) — the unfixed half named

| # | Done | Not done | Crates a fix touches |
|---|---|---|---|
| 8 | Clipboard **inbound**: `editor.rs:390` parses `input.html` via the untracked `clipboard_html.rs` | **Outbound**: no `copy`/`cut` handler for the document editor at all; nothing in `opendoc-render` | render + api + app + `src` — **4 owners** |
| 12 | Find/replace **in Rust** handles `Region::{Header,Footer,Footnote}` | **Navigation**: `AppFindMatch` carries only `EditorPosition`, so stepping onto a header match silently does nothing. Header/footer still edited only through a modal | api + app + `src` |
| 16 | Style list: `ieee` no longer double-listed, `apa-7th` no longer exempt | **Delivery**: `citation_support_warnings` is called only from import/export; nothing in `opendoc-app` | app + api + `src` |
| 17 | `export_google_sheets_json` returns `AppExport` with warnings | **CSV/XLSX still return `String`**; the contract declares it too | api + app + `src` |
| 18 | Service: authorization, quota, `.expect()` out from under the registry lock, `close_sessions_for_subject` called, `cursor_anchor` bounded, comment authors bound to the subject, `Action::Comment` live — **six of seven**, and now all mutation-proved | **Threads are bounded, not reclaimed** — and that is now a reasoned decision, not an oversight: the registry hands out handle clones, so dropping an entry while a clone lives would let a second thread spawn for the same branch head. A thread *does* exit when every handle drops; only the registry's own handle is immortal | **opendoc-service alone** |

### Still open (2)

- ~~**undo can *add* text**~~ — **FIXED 2026-09-14**, ADR 0017 amended.
- ~~**The signature mechanism is built but nothing calls it.**~~ **FIXED
  2026-09-14.** `sign_current_repository_version_with_openssh_private_key` is
  an explicit app/API command: it refuses unsaved work, signs the recorded
  manifest after it exists, and writes the coverage/signature sidecars. Open
  verifies those sidecars and audits their named ancestry; unreadable sidecars,
  failed verification and an incomplete chain are warnings, never an access
  gate. The end-to-end regression verifies the sidecar against its manifest and
  proves a subsequent unsaved edit cannot be signed as that version. Crates:
  app + api.
- ~~**A table-level border on `BlockKind::Table`.**~~ **FIXED 2026-09-15.**
  `TableProperties::border` distinguishes no document statement from an
  explicitly borderless table. It is a merge operation and app/API command,
  projects through the editor and layout/PDF, and round-trips through DOCX,
  ODT and the OpenDoc Google-shaped JSON extension. A uniform Word table grid
  now stays one table property; non-uniform grids remain per-cell overrides.
  Crates: core + merge + app + api + render/layout + import + service.
- ~~**NEW: `apply_batch` cannot model the merge's *other* set-wide deferrals.**~~
  **FIXED 2026-09-14.** `batch_inverse_capture_order` now lives beside the
  merge's deferred passes and the app's inverse-capture fold consumes it. It
  retains ordinary gesture order, then models suggestion resolutions, comment
  restores and mark ranges in the order the batch merge actually applies
  them. The regression uses all three deferred classes, so this cannot quietly
  collapse back to input order. Crates: app + merge.
- (was) **NEW, and a correctness bug: undo can *add* text.** A batch containing an
  `InsertText` followed by an `UpdateInlineText` on the same run captures an
  inverse against a state the document was never in, because the scratch fold
  merges one operation at a time and ADR 0007's whole-run reset never applies.
  Found at seed 136 by a generated undo test, reproduces on the pre-change tree.
  Needs an ADR 0017 decision about whole-run resets inside a batch, not just a
  code fix. Crates: app + merge.

- **13. Comments and suggestions are run-granular.** `TextRange { start: StableId, end: StableId }` is two *inline run* ids with no character offsets, and every anchor consumer follows. Crates: core + merge + app + render + api — **five**, the widest item on the list. Honest today: no test falsely claims character granularity.
- **20. A signature covers the snapshot only.** The signed payload is `SnapshotRecord::new(uuid, format, document)` and nothing else — no manifest, parent, segment list or blob-content digest. History can still be truncated and the version still verifies, which ADR 0002 and ADR 0003 both describe as covered. Crates: app + store + format + sign — **four**, plus the ADR text. No test claims to cover it: every signature test asserts tampering *within* the snapshot payload, which is exactly the scope not in question.

### Newly found, not on the original list

1. **The `editor.rs` quadratic is still there, at both sites** (`:1390`, `:1485`): `find_block(...)` inside the `boundaries` loop, so a select-all mark is O(n·m). Wave 1 recorded it as "new, found while fixing" and **it was never assigned**. Same shape as the find bug that cost 22 ms per keystroke. Single owner: `opendoc-app`.
2. **Every new document defaults to a style that warns.** `citation.rs:20` sets `style: "apa-7th"`, which wave 3's own fix made non-exempt — so the default document now trips `citation_support_warnings` on every export. Correct behaviour, wrong default.
3. **`Comment::author` is client-supplied *locally*.** `annotation_commands.rs:465` takes `author` as a command argument and writes it into signed state. The service binds it to the session subject, so the hole is only in local/offline documents — **which is the majority runtime**. Same severity as the service item that was fixed, one layer lower.
4. **The DOCX invented border grid** needs a table-level border property in `opendoc-core`, so it is *not* an `opendoc-import`-only job the way the wave 3 assignment table assumed.

### Caveats the audit put on itself, which I am keeping

- **The tree moved under it.** Item 15 went from open to fixed between two reads twenty minutes apart. Line numbers shifted by ~70 in one file in that window.
- **Item 15's tests are not there yet.** The deduplication is real; the `#[cfg(test)]` module in the new `lists.rs` was still empty at read time. **Verify the tests exist before closing it** — that work was still in flight.
- **Four of the files carrying decisive evidence are untracked.** Any tracked-files-only re-check reaches the *wrong* conclusion on items 5, 8 and 15.
- The audit ran no `cargo` command, so it did not confirm that the in-flight item-15 state compiles.

### What this means for scheduling

Only **item 18** is single-owner. Items 13 and 20 cannot be given to one worker
under exclusive crate ownership at all — they need either a serialized wave or
a different ownership model.

---

## §7. Why the gate was green

This is the finding that explains the others.

**The convergence suite proves nothing about convergence.** `merge_operations`
folds streams into a `BTreeMap<OperationId, Operation>` **before any semantics
run** — the code comment says so: *"Not a function of stream grouping or
arrival order."* Permutation-invariance is therefore true by construction, and
roughly **89 assertions** of the form "merge(grouping A) == merge(grouping B)"
hold for any implementation. Replace `causal_order`'s body with
`(0..len).collect()` — abandoning causality entirely — and all twelve
permutations still agree byte-for-byte, and the `divergent_from_base > 90%`
guard still passes.

Both the security and collaboration auditors reached this independently: the
collaboration auditor's own scratch fuzz, which carried an **oracle**, is what
found P0-2. The right oracle already exists in the repo (`converged_text` pins
the exact string) and is used by a minority of tests.

The same pattern elsewhere:

- **Canonical CBOR is never checked for canonicality** — encode-twice plus
  round-trip only. Switch `to_canonical_vec` to `cbor2::to_vec` and every test
  stays green, while two builds would fork a repository. No golden-byte
  fixture exists for any record type.
- **`assert!(bold.lines >= regular.lines)`** is satisfied by equality — that
  is, by exactly the bug it is meant to catch.
- **Checklist glyph tests** never tie a glyph to a state; swap the constants
  and every done item exports as an empty box.
- **Page-setup tests** assert only that `--page-width` is *declared*; hardcode
  Letter and every export ignores the document's page setup.
- **~106 `validate()` calls are dead**, because `merge_operations` already ends
  with `document.validate()?`.
- **`opendoc-spreadsheet` has 52 tests for 17,397 lines**, with **zero** in
  `functions.rs`, `formula.rs`, `recalc.rs`, `lookup.rs`, `format.rs` or
  `address.rs` — ~13,000 lines including the whole 258-function engine. Every
  bug in P0-7 was found in one afternoon.
- **`opendoc-render` has one escaping test, over plain text** — which is why
  P0-1 survived.
- **`src-tauri` is excluded from the workspace**, so its 7 real socket tests
  have **never been run by any gate**, and nothing lints the native shell. CI's
  `cargo check` does not even type-check the test file.
- **The e2e harness only rebuilds the service binary if it is absent** — so it
  can test yesterday's binary and pass. A gate that lies.
- **The ADR 0014 agreement fixture** is plain paragraphs, one heading and a
  bulleted list — no marks, no citations, no mentions, no marker change, no
  table. Widening that one fixture would have caught P1-1 and P1-2 on the day
  they landed.
- **67 of the 114 declared UI actions are never triggered** by any harness.
- **There are no fuzz targets** (`cargo-fuzz`, `proptest`, `arbitrary` are all
  absent); the `*fuzz*` files are hand-written property tests. None exercises
  deep nesting, decompression ratio or extreme dimensions — which is why P0-5
  survived.

### What to do about it

1. **Give every convergence test an oracle.** `converged_text` exists. A test
   that asserts two computations of the same function agree is not a test.
2. **Golden-byte fixtures for every canonical record**, since the format is
   content-addressed and a drift forks repositories.
3. **A golden `(formula, expected)` table** for all 258 functions.
4. **Put `src-tauri` in the gate** — two lines in `native-check.mjs`, and
   `native-check` itself into `npm run verify`.
5. **Always rebuild the service binary** in the e2e harness.
6. **Widen the layout agreement fixture** to cover every inline kind.
7. **Add real fuzz targets** for the file parsers.
8. **Grep the suite for assertions that compare a function against itself**,
   and for `>=` where `==` is meant.

---

## Coverage of the audits themselves

Stated so this file is not mistaken for completeness. Text ~75%, rendering
~80%, spreadsheet ~45% of the formula engine (≈120 of 258 names sampled) and
~75% elsewhere, collaboration ~75%, storage ~65%, shell/security good on the
shell and parsers, thin on `opendoc-store`/`opendoc-pdf`/`opendoc-citations`.

Largest untouched areas: the annotation and citation **merge** modules,
`opendoc-store`'s candidate-head reconciliation, `opendoc-sign`'s internals,
DOCX revisions/styles/footnotes/comments, and the trig/distribution tail of the
formula engine. **The audits found this much at that coverage.**

---

## Fix progress

### 2026-09-13: the typing path (the item that prompted this audit)

**The premise was partly wrong, and the measurement said so.**
`layout_document` was **2.35 ms, not 4.3** — about 9% of a keystroke. The
dominant cost was `refreshFind` at **22.3 ms**, and only with the find bar
open, because it early-returns when closed.

And `refreshFind` was not a scheduling question — it was **an O(n²) bug**.
`DocumentIndex::text_of` re-found each block by scanning the document from the
top, inside a loop over every block: 1.37 / 5.33 / 19.07 ms at 750 / 1,500 /
3,000 blocks. With an id→block map built once per search: **0.38 / 0.87 /
1.96 ms**. Deferring it was rejected on correctness grounds — a stale "3 of 5"
is worse than a millisecond.

| | before | after |
|---|---|---|
| keystroke with the find bar open | 49.0 ms | **26.2 ms** |
| `find_in_document` | 22.3 ms | **1.7 ms** |
| `layout_document` | 2.35 ms | **1.4 ms** |

**Incremental layout was added, against ADR 0014 — and the ADR's objection was
answered rather than overridden.** 0014 objected to the *key*: "a cache keyed
on anything less than everything that decides a fragment would trade this
crate's one guarantee". `LayoutCache` **has no key**. An entry stores the
inputs themselves — the whole `Block` by value and the `Frame` — and is reused
only when they compare `==`. No digest to collide, no field a future `Block`
variant could add unnoticed. The run state 0014 named as the hard part is not
in the key because it is not in the memoized unit: the marker glyph, ordinal
and trailing margin are applied to what comes back on every pass.

Worth it because comparing all 1,500 blocks costs **0.037 ms** against a
1.23 ms full pass — 33× cheaper. Byte-identity evidence: 180 generated
documents × 20 edits, comparing cached against uncached **after every edit** —
3,600 whole-layout comparisons. One mutation (no by-id fallback) **survived its
first test set**, which is why `a_block_that_moved_is_found_where_it_went`
exists.

Scope respected: it did **not** touch `Engine::text_fragment`'s leading or
`push_inline`, so P1-1 and P1-2 are untouched and now faithfully cached. It
states plainly that its byte-identity test compares cached against uncached,
**not against Chrome**, and makes no claim that layout matches the browser
beyond what the existing fixture covers.

#### The new dominant cost, and a familiar shape

`apply_editor_input` is now **13-15 ms** of a keystroke, of which
`get_document` is 5-6 ms. And `editor.rs` calls `find_block` inside a loop over
the selection at two sites — **the same quadratic shape as the find bug, in a
different file**, making a select-all mark quadratic in the selection.

Also recorded: the layout command serialises **178 KB of placements per
keystroke**, and `top_twips`/`height_twips`/`lines`/per-block `exact` have no
consumer outside the DTO — trimming them would cut it ~65%, but it is a
contract change for ~2.5% of a keystroke.

---

## Fix wave 1 complete — 2026-09-13

Full gate green: **1,192 tests passing / 0 failing** (1,098 at audit time),
clippy `-D warnings` clean on the workspace **and on `src-tauri`**, fmt clean,
no contract drift, typecheck clean, smoke passing, **e2e 49/49**, native-check
passing, `src-tauri` 22 tests passing.

**All nine P0s are fixed**, plus four P1s and several P2s.

### What the fixes changed about the findings

- **P0-1 (XSS)** — fixed at three independent layers, each verified to hold
  alone, and **confirmed live in WebKitGTK**. The sanitiser re-parses
  math-core's output and re-serialises from an allowlist rather than trying to
  escape it. It caught something the audit missed: math-core emits a bare
  `id="foo"` for `\label{foo}`, which could name `#app`.
  Trap found: a plain `cargo build --release` runs the shell in **dev** mode
  with `devCsp` unset, so the CSP only applies to a real `tauri build`.
- **P0-2 (table cells)** — the binding lives in the **operation**, not the
  document, so `TableCell` gains no field, `Document` stays byte-identical in
  shape and still `Eq`, and render/import/layout/pdf are untouched. A cell
  whose column is gone is **dropped**, because that is what makes "insert then
  delete" and "delete then insert" agree. ADR 0019.
- **P0-3 (signing)** — replaced with an **exhaustive `into_source_state()`**
  naming every field, so "did we remember to strip this?" is now a compile-time
  obligation. It found more than the audit: `page_layout`, `body_fragments`,
  the rendered HTML fields, and `blob.available` — which is set from store
  presence, so **a temporarily missing blob used to break the signature**.
  Also: making a broken signature *openable* without fixing the save path
  would have turned "cannot open" into "cannot save".
- **P0-6 (spreadsheet)** — one cell edit at 1,000 rows went **2,402 ms →
  28.8 ms**, the DTO **7.55 MB → 0.83 MB**. Three quadratics, not one; the
  second (`ensure_axis_metadata`) was also the engine behind the XLSX DoS.
- **P0-7 (formulas)** — the golden table found **four more** wrong answers
  while being written, one reaching users constantly: Rust's `{:.n}` rounds
  half to even, so `TEXT(2.5,"0")` gave `2`, affecting every formatted number
  in the grid and in CSV export.

### Corrections to this file's own premises

- The typing-path brief said `layout_document` cost 4.3 ms. **It was 2.35 ms.**
  The real cost was `refreshFind` at 22.3 ms — and it was **an O(n²) bug**
  (`DocumentIndex::text_of` re-scanning from the top inside a loop), not a
  scheduling question.
- An earlier claim that `build-wasm.mjs` retained 1.89 MB of symbol names was
  **wrong** — a misreading of its own log, repeated by me without checking.
- The Chrome question in the image bug had **two hypotheses and neither was
  right**: a click on a `contenteditable="false"` figure does not move the
  selection at all. Clicking an image and pressing Backspace deleted the last
  character of the *previous paragraph*, and the image was unreachable for
  Format ▸ Image size too.

### New, found while fixing

- **`editor.rs` has the same `find_block`-inside-a-loop quadratic** as the find
  bug, at two sites, making a select-all mark quadratic in the selection.
- **`apply_editor_input` is now the dominant keystroke cost** at 13-15 ms, of
  which `get_document` is 5-6 ms.
- **`projection_service` clones the whole workbook** per projection — now the
  largest remaining spreadsheet cost.
- Making the per-operation scratch merge fatal inside `apply_batch` was
  **wrong**, and only the e2e caught it: a batch is atomic, so a state halfway
  through one can be invalid while the whole is fine. Inverses captured after
  such a failure are now recorded as irreversible rather than trusted.
- The `CollabStatus` drift the security audit predicted **actually happened**:
  the browser gained a `selection` field the native shell lacked, and the
  presence-only pinning tests could not see it. Closed by hand.

### Process cost, recorded honestly

Two agents collided through a **shared scratchpad path** — one ran the other's
`mutate.py` against the **live tree**, leaving a real mutation in
`functions_legacy.rs`. It was caught, restored and reported unprompted, and the
affected agent discarded its entire mutation run and redid it against a private
copy. A killed e2e run also orphaned a Chrome that the next run silently
attached to, inheriting its IndexedDB.

The e2e harness derives `SERVICE_PORT = PORT + 1`, so agents on adjacent ports
collide even with CDP and HTTP ports set apart.

## Fix wave 2 complete — 2026-09-13

Three agents plus the coordinator. Gate re-run in full: **1,336 Rust tests,
0 failing** (opendoc-layout 90, opendoc-merge 220, opendoc-render 70,
opendoc-pdf 25, opendoc-service 92); `clippy -D warnings`, `fmt --check` and
`generate:commands --check` clean; `build:wasm`, `typecheck`, `build`, `smoke`
pass; `native-check` 24 Tauri shell tests; **e2e 62/62**.

### P1-1 and P1-2 — the layout divergences

**P1-1 is fixed by a different model than the finding proposed.** "Take the
largest size on the line" would not have closed the mono gap. A Chrome line box
is the union of the block's strut and every inline box on it, and Blink applies
three separate quantisations each worth at least a pixel: whole-pixel
ascent/descent, the used `line-height` on the 1/64px `LayoutUnit` grid, and
half-leading added to the ascent and floored with the descent taking the
remainder. All three are now modelled in `Fonts::line_extent`; `text::Breaker`
carries an extent beside every width so a word carried to the next line carries
its extent too. Every expected number in the new tests is a measurement taken
from real Chrome 147 against the bundled faces, not a derivation.

Two consequences fell out. Super/subscript shift is exactly
`LayoutUnit(size)/3 + 1px` (`/5` for sub) derived from the *parent's* size, so
`EstimateReason::RaisedText` is deleted — the value is computable and emitting
"estimated" for it was wrong. And a `Size` mark now beats a script mark's size
reduction, matching CSS specificity rather than mark order.

**P1-2 is fixed by deleting the padding, not by projecting it.** The finding
assumed layout should learn the 2px/2px/6px. It should not: nothing in the
document model says a code run is wider, and ADR 0014 makes layout a function of
`Document` and `PageSetup` alone. Projecting it would let a *theme* decide where
pages break. `standalone.rs` never had the padding, so removing it makes three
surfaces agree instead of adding a fourth copy of a constant.

### §7 — the section this file exists for

The ADR 0014 agreement fixture now carries every inline kind (code, size,
super/subscript, underline, strike, colour, link, mention, citation, footnote
ref) and every block kind, built by driving the real UI, with a separate check
that the fixture *contains* them all before any geometry is measured — a fixture
that silently failed to build is the same failure over again.

**The decisive evidence:** with the old one-leading-per-block arithmetic
restored, the *original* plain-paragraph check still passes and the *widened*
one fails. That is §7's claim demonstrated rather than asserted.

The two §7 `>=` tests in `opendoc-layout` were rewritten to the
`if differed > 0` family-of-inputs form their siblings already used.

### P1-8 and P1-9 — collaboration

The submit cap now travels in the **welcome frame** instead of being restated on
three sides, and a rejection resynchronises instead of wedging the session
forever.

### Item 8 — Enter in a table cell

Root cause was not in the editor. `opendoc-merge/src/blocks.rs::insert_block`
resolved its anchor only in `document.blocks`, so
`InsertBlock { position: After(cell block) }` silently degraded to appending at
the end of the *body*, and `split_block` fell back to a soft break.
`delete_block_in_blocks` already descended into `row.cells[].blocks`; insert did
not. Split into three functions with a recursive
`insert_block_beside_nested_anchor`. Mutation-proved: reverting the descent
fails with "the inserted block escaped the table and landed in the body".

### Corrections to premises, wave 2

- **My xlsx diagnosis was half right.** I named one panic site; there were two,
  and both proof-of-concept files still panicked after the first fix. The second
  is an index into an empty shared-string table. `catch_unwind` is not a remedy
  here — it catches nothing on `wasm32` — so the package is pre-flighted against
  the panic's own precondition instead.
- **I told the user `build-wasm.mjs` retained 1.89 MB of symbol names.** It does
  not; I misread the build log. The current run reports 1.96 MB of names
  *dropped*.
- The fuzz crate's mirrored limit constants had **already drifted** (1,024 vs a
  real 1,000) and now re-export the real ones.

### Cannot-fail tests found while fixing, not by the audit

One of the spreadsheet agent's *own* new tests survived its own mutation: a
merged-cell summary counted 1 either way, because the new clear-covered-content
behaviour had emptied the cell. Exactly §7's pattern, caught only because the
mutation was actually run. Rewritten against the state that really occurs.

The inline-padding test summed run widths, so it could not see a pad box at all
— the one mutation that escaped an 18-mutation campaign. Rewritten to assert run
x-positions; it now catches leading pads, trailing pads and citation pads.

### Process cost, wave 2

The shared scratchpad collided **a second time**. The layout agent's first
mutation campaign ran against the **live checkout**; it disclosed this
unprompted, discarded all of that evidence, and re-ran all 18 mutations against
an `rsync`'d private copy with its own `CARGO_TARGET_DIR` and a uniquely-named
script that refuses to run outside the scratchpad (18/18 caught). The
collaboration agent disclosed the same class of problem with a consequence I had
not considered: the e2e harness **builds `opendoc-service`**, so another agent
running e2e during that window could have tested a mutated binary. Both windows
were verified clean afterwards.

Generic names really do collide there — the scratchpad root holds `mut.py`,
`mutate.sh`, `mutate.log`, `mutation-results.json`, `e2emut.sh` and `wasmmut.sh`
from several agents at once. **Any future mutation campaign must use a private
copy of the tree and a uniquely-named script.**

The e2e harness also previously built `opendoc-service` only
`if (!existsSync(binary))`, so a stale binary passed silently — a gate that
lies. It now always builds.

## Fix wave 3 — in progress

### Wave 3 assignment — strictly disjoint crates

Six workers, no shared file except `e2e.mjs` (append-only, re-read before each
edit) — after a duplicate declaration there broke `node --check` in wave 2, and
after three separate live-tree mutation collisions.

| Worker | Owns | Items |
|---|---|---|
| A | `opendoc-merge`, `opendoc-format`, `opendoc-pdf`, `opendoc-store` | §7's remainder: the tautological convergence suite, canonical CBOR never checked for canonicality, and the cannot-fail assertions in those crates |
| B | `opendoc-import` | P1-4 DOCX tables — import misaligns columns, export drops spans/widths/borders silently |
| C | `opendoc-app` | P1-5 restore desynchronises the snapshot, P1-6 recovery discards a signature, P1-7 two runtimes permanently kill crash protection |
| D | `apps/desktop/src-tauri/src/fetch.rs` | P1-11 the check-then-connect SSRF guard |
| E | `opendoc-service` | no authorization or quota on document creation, threads never stopped, `.expect()` **inside the registry lock** (a transient exhaustion becomes permanent), dead revocation, unbounded `cursor_anchor`, client-supplied comment authors, the `Commenter` role that cannot comment |
| F | `opendoc-layout`, `opendoc-render` | ordered-list numbering implemented twice and disagreeing |
| coordinator | `opendoc-wasm`, `apps/desktop/src` | the two-tab storage clobber, and the discarded `storage_ready` report |

`opendoc-render` already depends on `opendoc-layout`, so F's deduplication has
somewhere to live without touching `opendoc-core`. I had assumed otherwise and
checked before briefing.

### P1-4 was already done — and the verification found a different bug

The third finding this wave that turned out to be fixed before anyone worked on
it. `crates/opendoc-import/src/docx/table.rs` — **untracked**, so invisible to
both the audit and `git status` summaries — already planned the grid before
converting cells, materialised covered positions, and read `w:gridCol`,
`w:gridSpan`, `w:vMerge`, `w:shd`, `w:tcBorders`, `w:vAlign` and `w:tcMar`; the
writer already emitted all of it with ADR 0010 named warnings. That file and
the new fixture are now **staged**: the crate does not compile without the
first and its tests cannot run without the second.

So the work became verification, and verification is what found the real bug.

**A genuine new defect, found only because the round trip was checked against a
foreign reader.** `w:tblCellMar` — the table-level default cell margin, which
LibreOffice writes on **every** table it produces — was read by nobody and
warned about by nobody. A cell stating no `w:tcMar` of its own lost its real
padding and OpenDoc substituted its own default. `plan_table` now seeds each
cell from `w:tblCellMar`, with a cell's own `w:tcMar` overriding per edge, which
is how WordprocessingML resolves the pair.

**The method is worth keeping.** A real LibreOffice 7.3.7.2 package — zip,
`[Content_Types]`, rels, `word/styles.xml` and all — is committed at
`crates/opendoc-import/fixtures/libreoffice-73-merged-table.docx`, not a body
pasted into a package this repo builds itself. Then the export was converted
*back* by LibreOffice and diffed against the ODT of the original: rows, cells,
covered cells, spans, column widths, backgrounds, vertical alignment and both
borders all identical. That diff is what surfaced `w:tblCellMar`, and no
self-round-trip could have.

**22 mutations, three of which survived the first pass** and each exposed a
real gap: `docx-clamped-table-column-width` had no test at all; a `w:vMerge`
continuation carrying `w:gridSpan` — the only case where the covered/orphan
logic changes the output — was untested in both directions. All three fail
correctly now.

`opendoc-import`: 206 → **212 tests**.

### New, from B: the export invents a border grid

Not a regression, and not fixed: the DOCX writer emits a
`single sz=4 color=auto` `w:tblBorders` grid on **every** table regardless of
what the model says, and `w:tblBorders` is dropped on import with no warning.
It is defensible — it materialises OpenDoc's own on-screen default — but it is
undocumented and unwarned, and it is the one remaining divergence in the
LibreOffice round trip (`fo:border="none"` comes back as `0.5pt solid #000000`).

The two halves collide, which is why neither was taken on: making the reader
materialise table-level borders onto cells would break round-trip identity for
every existing table test, precisely because the writer invents that grid
unconditionally. The coherent fix needs a table-level border property in
`opendoc-core` — or a deliberate decision that borderless exports are allowed.
**Its own item.**

### Fixed, but nothing could have told you — the P1-11 result

This is the most important thing wave 3 found, and it changes what "already
fixed" is worth.

`guard_fetch_url` was gone before agent D started. `fetch.rs` already pinned
`resolve_to_addrs`, already called `no_proxy()`, already ran a manual per-hop
redirect loop, and had already closed all four `is_private_address` gaps. The
fifth item this wave that was fixed before anyone worked on it.

**But the one test that claimed to establish the property —
`the_screened_addresses_are_the_ones_the_client_is_pinned_to` — asserted only
`client.is_ok()`.** It could not fail. The security property lived in comments.
A later refactor could have reintroduced check-then-connect and the gate would
have stayed green, which is exactly the shape §7 exists to name.

So the four earlier "already fixed" verdicts — P1-3, P1-4, P1-10, `strict`
validation — carried a caveat I should state rather than leave implied: I had
verified those by **reading the code**, not by establishing that a falsifiable
test guards them. D has just demonstrated those are not the same claim.

I went back and checked the two I had claimed on my own authority:

- **P1-10 is properly guarded.** `an_image_pixel_attestation_is_bound_to_the_bytes_it_names`
  signs a PNG, transplants the signature onto WEBP bytes, and requires
  `"broken"` — the exact attack the finding described, and it would fail if the
  binding were removed.
- **P1-3 is guarded twice.** `a_mark_reaches_every_text_block_across_a_page_break`
  and `a_partial_mark_across_an_image_uses_each_blocks_own_boundaries` both put
  a non-text block inside the selection, and the second pins the precise defect
  (each block reading *its own* boundaries). The e2e check "bold reaches every
  paragraph across a page break" covers it from the browser as well.

P1-4's guard is agent B's 22-mutation campaign. `strict` validation is the one
still taken on code-reading alone; the read-only sweep now running is asked to
settle it.

The test that now holds the property is worth describing, because the obvious
version of it would not have worked. `a_name_the_client_could_look_up_again_never_reaches_the_socket`
uses the host name **`localhost`**: the scripted resolver the screen sees
answers a *public* address, while the real system resolver answers `127.0.0.1`,
where a listener is actually bound. Any second lookup by anyone — reqwest's own
resolver, a proxy, anything — lands on that listener, and the assertion is the
listener's connection count. Deleting the `resolve_to_addrs` pin (the finding in
its original form) fails it with:

> the fetch connected to the listener on 127.0.0.1, which is not the address the
> screen passed: the host is being resolved a second time, so the screened
> address is not the connected address — left: 1, right: 0

### A correction to the finding's proposed defence

PLAN88 says a configured proxy defeats the guard "because reqwest is built with
the `system-proxy` feature", which implies turning that feature off is a
defence. **It is not.** `hyper-util`'s proxy matcher reads `HTTP_PROXY` /
`http_proxy` / `https_proxy` **unconditionally**; `client-proxy-system` only
adds the macOS/Windows system-settings readers on top, and reqwest's
`auto_sys_proxy` defaults to true. A client built without `.no_proxy()` would be
proxied on this host whatever the feature flags say. **`.no_proxy()` at the call
site is the entire defence**, the `Cargo.toml` comment that claimed otherwise
is corrected, and a test now holds the real guarantee. Feature unification was
checked too (`opendal` also pulls reqwest) — nothing re-enables it.

`native-check`: 24 → **32 tests**. 29 mutations, 28 killed.

**Round 1 caught two of D's own tests that could not fail**, both of the same
family §7 names: `a_redirect_loop_ends` derived *both* of its expectations from
`MAX_FETCH_REDIRECTS`, so raising the constant moved the test along with it
(now hard-coded 5/6); and the `2001::/23` clause was only ever exercised by a
Teredo address whose second group is zero, so narrowing `/23` to `/32` survived
until both sides of the boundary were pinned.

One mutation survived and D reported it as an **equivalent mutant** rather than
papering over it: passing port `0` to the resolver changes nothing observable,
because reqwest documents that the URL's port always wins and screening looks
only at the IP. Reporting it beats writing a test that would merely re-assert
the mutation is harmless.

### The service — six of seven already fixed, but unproven — the E result

Same pattern as the rest of the wave: six of the seven service findings were
already fixed in the working tree. The difference here is that **none of that
work carried mutation evidence**, so E treated it as unproven and supplied
**13 mutation proofs** rather than accepting green as correct. That is the right
instinct given P1-11, where the code was right and the proof was hollow.

**Finding 3 was half right, and the half that was right is the dangerous one.**
PLAN88 states the `.expect()`-under-the-registry-lock as a property of
`POST /v1/documents`. It was not. In `create_document` the spawn was *outside*
the lock, so exhaustion there merely errored. The `.expect()` was in
`OpenDocService::document()` — **the path every join takes**. So the permanent
lock poisoning was reachable by any client opening any document, not only by
document creation. Worse than reported, and in a different place.

The mutation that proves it is the one I would point at: restoring the historical
`panic!` under the lock makes the test fail **twice** — first the panic itself,
then `harness.service.document(&first).is_ok()`. The test sees the *poisoning*,
not merely the error type, which is exactly the property that was asked for.

**A real bug found while verifying, not while fixing.** `create_document`
charged the durable quota and wrote the genesis commit **before** checking
whether the registry had room for a thread. At the open-document limit the
caller got an error while an owned, durable document existed and the quota had
already been charged. The capacity check now runs before anything is written,
through the same helper as the insert-time check — one implementation of the
rule, because a second copy let a mutation survive.

**Two tests were passing for the wrong reason**, and this is the wave's clearest
example of why mutation testing is not optional:

- `a_commenter_may_comment_and_may_not_type` failed a mutation with
  `left: "conflict", right: "forbidden"`. The Commenter's keystroke was being
  refused as a **history rewrite** — the session had not drained its own commit,
  so the client reused sequence 1 — not as a permission failure. **The test
  would have gone green against a service that let Commenters type**, as long as
  the client reused a sequence.
- `a_service_at_its_open_document_limit_refuses_and_keeps_working` asserted the
  refusal but nothing about side effects, so the quota bug above was invisible
  to it.

**Finding 6 was the one genuinely open, and the fix is a judgement worth
recording.** `Comment::author` is free text *inside* the operation payload, so
the actor binding never covered it: bob's session is honestly bob's, the
`OperationId` is honestly bob's, and the comment still said alice wrote it —
permanently, inside signed source state. E **bound it rather than rewriting
it**: a batch whose author field disagrees with the session subject is refused.
Rewriting would have left the operation the server logs different from the one
the client holds *under the same id*, with nobody told — divergence in the one
place nobody looks.

Author extraction shares the exhaustive `match` that decides `Action::Comment`,
so a new `OperationKind` is a **build failure** rather than either a permission
hole or an unchecked author field.

`opendoc-service`: 92 → **93**.

#### The break this creates, and who owns it

`opendoc-app` still takes the author from the caller
(`annotation_commands.rs:406` and siblings), so **a comment made in the app
during a live session is now rejected by a real service**. I confirmed nothing
would catch it: no test in `app_client_tests.rs` drives a real `OpenDocApp`
adding a comment over a socket, which is why 93 tests are green and this is
still broken. Assigned.

#### Left open deliberately, with reasons

- **Document threads are bounded, not reclaimed.** Eviction is not an oversight:
  the registry hands out handle clones, and dropping an entry while a clone
  lives would let a second thread spawn for the same branch head. A document
  thread *does* exit when every handle drops; only the registry's own handle is
  immortal.
- `UpdateCommentBody`, `DeleteComment`, `DeleteCommentThread` and the restore
  pair carry **no author at all**, so a Commenter can still edit or delete
  **another subject's** comment. Closing it means reading the existing comment's
  author out of the materialised document, and it is a policy call — an Owner
  presumably may delete anyone's.
- The outbound per-connection queue is still unbounded; the anchor cap shrinks
  the per-frame cost but does not bound the queue.
- `CreateDocumentRequest.title` is bounded only by axum's 2 MB body limit, and
  that title goes into signed state.

#### ADR 0015, edited beyond what I authorised — and rightly

E edited `docs/adr/0015` past the revocation clause I had authorised, disclosed
it unprompted, and gave the reason: its change made the amendment paragraph
headed *"Still trusted to the client, unchanged: the author field inside a
comment payload"* **false**, and that paragraph is a statement about a security
property. Leaving a documented security claim that the code contradicts would
have been the worse outcome. One contiguous hunk, trivially revertible. Correct
call.

### List numbering — one implementation, and a third defect nobody had found

My briefing was right on both counts and **incomplete on a third**. Beyond
bullets consuming ordinals and the two implementations keyed differently,
`ListWriter` restarted its counter when `list_id` changed but did **not close
the wrapper** — so two adjacent lists at the same level shared one `<ol>`, and
the renderer wrote `value="1"` on an item Chrome calls `4`. That made the markup
self-contradictory in the *opposite* direction from the reported bug, and the
layout did not restart there at all. A genuine render/layout divergence that
existed independently of the one that was reported.

**Measured, not reasoned.** Chrome 147's accessibility tree gives a list item's
marker box as an `AXListMarker` whose name is the string Blink painted. The old
export of a mixed list wrote `value="3" value="4" value="5"` where Chrome, given
the *same markup with `value=` stripped*, counts `1 2 3`.

**The design decision is better than the one I proposed.** I suggested "reset
the ordinal when a wrapper opens". F went further: *the ordinal belongs to the
wrapper, not to a `(list, level)` pair*. That deletes the keying rather than
patching it, and both symptoms stop being **expressible** — a bulleted item
cannot consume an ordinal because a wrapper holds exactly one marker, and a
reopened `<ol>` restarts because its count went with the element that closed.
`list_id` became part of wrapper identity for the same reason, which is what
fixes the third defect.

It also kept writing `value=` rather than letting the browser count, and made
the invariant testable instead: **the number we write is the number a browser
would count unaided.** That is a stronger statement than "the two Rust
implementations agree", which two copies of the same wrong rule would satisfy.

One more duplicate removed on the way: `standalone.rs` hand-wrote the `depth-N`
`list-style-type` rules with a comment saying they were kept in step with
`opendoc_layout::STYLED_LIST_DEPTHS` — a second copy maintained by a comment. It
now calls `list_style_type_rules`, and the generated text is byte-identical.

`opendoc-layout` 90 → **100**, `opendoc-render` 70 → **74**, `opendoc-pdf` 25
unchanged.

**16 mutations, all caught — and three survived the first campaign**, each
exposing a real weakness:

1. `STYLED_LIST_DEPTHS 9 → 12` survived because the cut-off test looped
   `for depth in 0..STYLED_LIST_DEPTHS` — **the expected value computed by the
   code under test, §7 in a brand-new test written by an agent who had been
   warned about exactly that**. Now a literal `[0,1,2,0,1,2,0,1,2,0,0,0,0]`, and
   the CSS test pins the whole rule block verbatim.
2. Constant-depth in `paint_list_marker` survived because nothing painted a
   *nested* ordered list. Now pinned to Chrome's measured `["1.","a.","b.","i.","ii.","c.","2."]`.
3. An unconditional `</li>` survived because no fixture had a wrapper that
   opens only to hold a deeper one and closes empty.

#### My e2e check, and its mutation proof

Appended *"the number Rust writes is the number the browser would count"*: for
every `ol.doc-list` in the editor host, each direct `li`'s `value` must equal
its 1-based position among its siblings, with a guard that an ordered list was
actually built so it cannot pass vacuously — plus the converse, that the
bulleted run became its own wrapper carrying no numbers at all.

Deliberately **not** a render-vs-layout agreement check. The DOM Chrome built is
the independent answer.

Mutation, in a private copy: start a new wrapper's ordinal at 2 instead of 0 —
the observable symptom of the original bug. The check fails with
`[{"stated":3,"expected":1,"text":"ordered one"},{"stated":4,"expected":2,...}]`,
i.e. the exact `value="3" value="4"` F measured the old code emitting. e2e is
now **65/65**.

#### Follow-ups F identified, correctly, as mine — and what I did with them

- **The app stylesheet is the last hand-written copy** of the `depth-N` cycle
  (`styles.css` ~857-878). Feeding it `list_style_type_rules` the way
  `type_scale_css_variables` already reaches `pagination.ts` needs a field on
  `AppDocumentLayout` — squarely inside the crate the contract-drift worker is
  rewriting right now. **Deferred rather than forced**, because starting it
  mid-flight would collide with the DTO work.
- **A measured screen/paper divergence, pre-existing:** a depth-1 bullet draws
  `◦` on screen and `•` on paper, because `Fonts::covers('\u{25E6}')` is false —
  `fonts/generate.py`'s `UNICODES` has `U+25A0` but nothing at `U+25E6`.
  Liberation Sans does carry the glyph. Fixing it means regenerating the bundled
  TTFs **and** the WOFF2 from one run — and the entire ADR 0014 agreement suite
  was measured in Chrome against the *current* WOFF2. **Not attempted at the end
  of a wave**; it needs its own re-measurement pass.

### The generated contract was not all generated — and the number was worse

I briefed this as "roughly forty" hand-written literals. It is **97** that
mirror a Rust type (89 projection objects, 1 argument object, 7 string-union
enums), plus 7 that legitimately mirror nothing. Every one was invisible to
`generate:commands --check`.

**54 real drift findings across 13 types**, found today — the literals were not
merely *capable* of rotting, they had already rotted.

**One functional bug, and it is the right example of the class.**
`OpenDocStorageBackend::OpenDalFs` serialized as **`"open-dal-fs"`** (from a
struct-level `rename_all = "kebab-case"`), while *every other spelling in the
workspace* — `as_str()`, the arg parser, `repository.rs`, `audit_view.rs` and
the TS union — says `"opendal-fs"`. So `OpenDocRuntimeProfile.storage_backends`
handed clients a string its own typed union excluded, the arg parser rejected,
and its own `Deserialize` could not round-trip.

It was **reachable in a shipping build and not feature-gated**: `for_mode` adds
`OpenDalFs` for every mode but `browser-local`, and the desktop shell ships
`features = ["opendal-store"]`. What masked it is that `main.ts` sends
`storageBackends: runtime.storageBackends ?? []`, which is `[]` with no host
config — and no frontend code ever *reads* the field. So: a latent contract
break that would have become a live bug the first time a host declared its
backends or the first consumer read the field. Nothing to migrate, since the
profile is a command result and never a stored record.

**The other 53 are optionality and nullability**, which is the interesting
shape: no field was missing, extra or renamed — the literals were
field-accurate. 11 fields were marked `?` that Rust *always* serializes; 35 were
typed `| null` that Rust *never* sends as null (`Option<T>` +
`skip_serializing_if` means **absent**, not null). `AppBlockProperties` was
internally inconsistent with itself — six fields skipped when unset, four wrote
`null`.

#### The check, and an honest note about which option was taken

The agent chose the **checked-literal** route, not genuine derivation, and said
so plainly. Derivation is blocked twice: `opendoc-api` cannot depend on
`opendoc-app`, and the file-writing binary lives in `opendoc-api`, so emitting
derived types would mean moving the generator and editing `package.json`.

What it avoided is the *weak* version of the cheap option. There are **no
hand-written sample values anywhere** — samples rot exactly like literals do.
Instead: one `Deserializer` that answers nothing and reports serde's own
`FIELDS`/`VARIANTS` (the authoritative post-`rename` names), and one that
*constructs* a value in two modes — every `Option` `None`, then every `Option`
`Some` — so serializing both yields, per field, exactly "can this key be absent"
and "can it be null". A second test refuses to let a new literal appear at all
without a registry entry or a stated exemption, and fails on stale exemptions.

**16 mutations, every one caught — and `generate:commands --check` stayed green
for all 16.** That is the finding restated as evidence: the existing gate is
blind to this entire class. Four mutations were caught by the compiler in round
one, so round two repeated them as edits that *compile*.

#### The adjacent hole, swept and clean

The same agent checked whether `CommandSpec`'s hand-written arg lists drift from
what the parsers demand, across ~300 commands: **zero args marked optional that
the parser actually requires** — the dangerous direction is clean. The 51 args
declared required but tolerated absent are all `NullableString` whose parser
reads a missing key as `null`: stricter metadata, not a bug. A permanent guard
was added, and it deliberately does not assert the reverse.

#### The signature consequence — visible, but **misattributed**

Making `AppBlockProperties` consistent re-encodes `AppDocument`, so an existing
saved document with block formatting now fails its stored signature check. That
does **not** land silently — `open_saved_projection` pushes
`broken-document-signature` naming the signer, and the next save drops it with
`dropped-broken-signature`.

But the message means *"source state moved under the signature"* — the tampering
case. Here nothing about the document moved; **the encoding moved**. A user
reading it concludes their document was altered.

The honest fix, handed over rather than rushed: `APP_DOCUMENT_FORMAT` is exactly
the constant that marks "the payload changed shape", so bump it to
`opendoc.app-document.v2`, widen `is_readable_snapshot_format` to accept v1, and
emit a distinct `signature-predates-payload-format` when the decoded snapshot
carries an earlier format. That separates *"someone changed this document"* from
*"we changed how we write documents"*. It touches the read path and wants its
own v1-read test. **Open.**

### Landing the author binding — the part that needed two crates at once

The agent implemented the annotation author binding, measured it, and
**deliberately did not land it**, because it took `opendoc-service` from 94 to
93: `app_client_tests.rs` asserted that an `OpenDocApp` given
`"author": "Bob Brown"` is *refused*, and once the app binds the author the
service accepts it. Correct call — it would have redded another live agent's
suite.

I landed both halves together once that agent finished. `annotation_author()` is
one chokepoint on `OpenDocApp` reading the existing public `service_session()`
accessor, used by all nine comment/suggestion sites. **With a session the
command argument is ignored entirely** rather than validated, so no caller can
get it wrong; **with no session the caller's name is used**, because a local
document has no attested identity to bind to and ADR 0004 already calls local
permissions advisory. That last part is irreducible, not an oversight, and it
means a local document still carries a caller-asserted author into signed state.

`accept_suggestion`/`reject_suggestion` were **left alone on purpose**: same
class, but the service's refusal rule covers `Comment::author` and
`Suggestion::author` only, so binding them would be a separate decision needing
its own service-side counterpart.

**The dying assertion was a duplicate, and the replacement already existed.**
`a_comment_attributed_to_another_subject_is_refused` proves the refusal against
a raw client building a forged frame by hand — it never goes through
`OpenDocApp` and is untouched by the binding. So rather than port the old
assertion, I **inverted** it into the stronger claim: hand the app a display
name, and the batch is *accepted* with the **service's own document** showing
both comments under the attested subject and neither under `"Bob Brown"`. The
two tests cross-reference each other, so that "restoring" the old expectation
would visibly mean weakening the binding.

Mutation-proved: making a joined session stop binding fails it with the
service's own words —

> the app bound the author to its subject, so this must be taken, got
> `Rejected { code: "forbidden", message: "operation actor-bob#2 attributes
> content to \"Bob Brown\", but this session is authenticated as \"bob\"" }`

Restored byte-identically (sha256 verified).

`opendoc-api` 14 → **16**, `opendoc-app` 301 → **307**, `opendoc-service` **94**.

### Superseded: the original framing of this finding

CLAUDE.md's central claim is "the contract is generated, never hand-written",
and `npm run generate:commands -- --check` is the gate that proves it. For the
command surface that is true. But **roughly forty `export type Xxx = { … }`
object literals in `generate_command_contract.rs` are hand-written strings that
mirror Rust structs**, and nothing checks that the two agree.

C found it while trying to add a field to `AppRecoverySession`, and the
consequence is precise: add a field to the Rust struct and it serializes to
clients immediately, never appears in `apps/desktop/src/generated/*.ts`, and
**`--check` stays green**, because the generator's own output did not change.
The gate cannot see this class of drift at all.

That is §7's shape applied to the repo's central invariant, and it is worse
than a tautological test because the whole project has been leaning on it. I
had already removed one stale literal from that file earlier (a dead
`invalidation_order: string[];`), which is direct evidence the literals do rot.

C correctly declined to add the Rust field, because adding it alone would have
*created* the silent drift rather than exposed it. Assigned as its own item.

### Disk: 131 GB reclaimed, and what it says about the release-only rule

The host was at **94% on `/big` and 99% on `/data`** when an agent mentioned it
in passing. Two separate problems, both worth recording.

**The scratchpad had grown to 100 GB.** Seven agents' private mutation trees and
their `CARGO_TARGET_DIR`s, every one belonging to a finished agent. This is the
direct cost of the rule I imposed after the live-tree collisions — "mutate a
private copy, with your own target dir" — and it is still the right rule, but it
has a disposal obligation attached that none of the briefs mentioned. The
14 small result files (204 KB total) are preserved under
`scratchpad/_mutation-evidence`; the 92 GB of trees and build caches are gone.

**`target/debug` was 39 GB** — on a project whose standing rule is *"only do
release builds; debug is slow and never beneficial"*, and whose every documented
gate passes `--release`. Nothing here consumes a debug artifact, so that was
39 GB produced entirely by tooling nobody asked for: a plain `cargo check`,
`cargo clippy` or `cargo test` without `--release` writes there, and so does
rust-analyzer. Removing it cost nothing — the workspace still reports 1,383
passing with no rebuild, because the release artifacts were untouched.

**Worth acting on next wave:** if agents are running bare `cargo check`/`clippy`
out of habit, the release-only rule is being followed in the *reported* gate and
violated in the working loop. That is invisible in every report, because the
reports only quote the release commands.

I first wrote here that setting it as a default in `.cargo/config.toml` would
fix it. **That is wrong, and I checked rather than leaving it standing:** on
stable cargo, `[build] profile = "release"` is an *unused config key* — it warns
and builds `dev` into `target/debug` anyway. There is no stable mechanism to
make `--release` the default.

So the rule is enforceable only by habit and by the documented gate, and the
practical mitigation is to sweep `target/debug` periodically. CLAUDE.md already
states the rule correctly; nothing there needs changing.

### Process cost, wave 3

The pattern-matching-your-own-process hazard has now appeared in **three
distinct forms**, and it is worth naming as one bug rather than three
accidents. A `-f` pattern is matched against full command lines, and the
command line of the process *doing the matching* contains the pattern:

1. `pkill -f <pattern>` killing its own shell (exit 144), earlier in the
   project.
2. An agent's cleanup used `pkill -f "rsync -a --exclude target"`, which matched
   its own shell **and another agent's in-flight `rsync`**. The other process
   survived by luck.
3. Seven background shells left over from earlier waves, each running
   `until ! pgrep -f mutate.py; do sleep 30; done`. The waiting shell's own
   command line contains `mutate.py`, so `pgrep` matched the waiter itself and
   **the condition could never become false**. They spun for up to 2h 45m after
   the mutation runs they were waiting on had finished. Stopped by PID.

**The rule: never use a `-f` pattern to find or kill processes you care about.
Use PIDs.** If a wait loop is genuinely needed, wait on the PID or on a
sentinel file, not on a pattern that names itself.

Mutation discipline held this wave — every campaign ran against a private
`rsync` copy with its own `CARGO_TARGET_DIR` and a uniquely-named script that
refuses to run outside the scratchpad, and the live tree was verified clean
after each. That is four waves of collisions finally stopped by making the rule
explicit in the brief rather than assuming it.

One coordination cost worth recording: `cargo fmt --all --check` is unusable
while a wave is in flight. It fails on whichever agent is mid-edit — at one
point on a `mod lists_tests;` whose file did not exist yet. Agents must check
`cargo fmt --check` on their own files only, and the coordinator runs the
workspace check after the wave lands.

### §7's own evidence had expired — the A result

My §7 claim was **structurally right and evidentially stale**, and the
distinction matters.

Right: `merge_operations` folds every stream into a `BTreeMap<OperationId, _>`
and then calls `causal_order` on `into_values()`, which is always
`(actor, seq)`-sorted whatever the grouping was. The result is provably a pure
function of `(base, set-of-operations)`, so every "grouping A == grouping B"
assertion genuinely cannot fail.

Stale: the specific evidence §7 cites — "replace `causal_order`'s body with
`(0..len).collect()` and all twelve permutations still agree" — **no longer
reproduces**. Run against the tree as it is, that mutation fails **9 tests**. A
prior wave had already given most of `opendoc-merge` real document oracles. Of
94 multi-merge tests, only **2** still had none.

The fix worth copying elsewhere: rather than only deleting the tautological
comparisons, A **kept a deliberately causality-blind merge in the test file**,
demonstrated that it is *perfectly* grouping-invariant, and asserted the real
merge disagrees with it. That turns "these assertions are tautological" from a
comment into something the suite enforces. The grouping comparisons stay, but
demoted in their doc comments to documentation rather than checks, and each
case now carries a **positive control**: every operation merged alone must
change the document, or the case cannot tell a merge from a no-op.

Canonical CBOR was half stale too — an **untracked** `golden_tests.rs` already
pinned the canonical bytes of all 9 records. What was genuinely missing is now
there: `scan_canonical`, an RFC 8949 §4.2.1 checker **written from the RFC
rather than from `cbor2`**, so it is an independent oracle and not a
restatement of the encoder. Three tests prove the checker can say *no* before
anything relies on it, and a negative control shows a merely-deterministic
encoder would not pass the module.

A finding along the way: `cbor2` 1.1.4's documentation says decoding "handles …
segmented strings", but its **serde** path — the one `decode_cbor` uses —
rejects them. Refusal is the right behaviour; being wrong about which it is, is
not, so it is pinned.

**Two corrections to my own §7 table.** My "`f(x) == f(x)` cannot fail" was an
overstatement: it *could* fail on intra-process nondeterminism. It is vacuous
where it matters, though — it passes for an exporter returning a constant and
cannot see a clock. And A found **two more of the same shape that I had not
listed**, both satisfied by `0 == 0`, i.e. by an export that embeds no font at
all. All now pinned to independently-known values.

18/18 mutations caught. merge 220 → **223**, format 34 → **44**, store 53 →
**54**, pdf 25 (3 rewritten).

### P1-5, P1-6, P1-7 were all already fixed — the C result

Three more. C's useful work was proving the fixes were real rather than
green-looking: four verification mutants, all killed, each restore sha256-checked.
The P1-5 replay test is honest — it replays the reopened repository's envelope
chain from genesis against the committed snapshot, not against itself, and
carries a pre-restore control so a broken walk cannot pass silently.

**What was still open was P1-6's second half.** The signature survives recovery
now, but the *offer* still could not say so: a sign-then-crash session was
offered as `operation_count: 0, operations: [], warnings: []`. The UI's "still
had 0 unsaved changes" was literally what the DTO said, while discarding
destroyed a signature. Now a named warning, `recovery-journal-unsaved-signature`,
owned by the offer set so discarding takes it along — with a **negative
control**, because without one an `if true` implementation passes.

**And P1-5's oracle was weaker than the production check it stood for.** The
test compared only `block_texts` while production's `documents_agree` compares
whole documents. Now whole-document equality, plus a fixture that changes only
a heading level, the title, the locale and the page setup — so the block text
is identical either side and only the structural half of the oracle can fail.
The mutation proving it also proves the old text-only assertion would have
passed.

`opendoc-app`: 298 → **301**.

### The native two-runtime case is not the browser one — and C said so

I asked C whether the Web Lock shape from ADR 0008 §6 mapped onto P1-7. It
argued that it does not, and I think it is right:

> Two tabs contend for *one* set of keys, so exactly one writer is correct and a
> memory-only tab is an honest downgrade. Two native runtimes write *disjoint*
> files — one segment per session id — and each is a real editor with its own
> unsaved work that deserves its own journal; making the second memory-only
> would delete crash protection for a legitimate window.

What collides natively is not the writes but the **offer**: a segment whose
owner is alive is indistinguishable from one whose owner died. So the native
equivalent of the lock is *liveness in the segment* — a heartbeat frame, and a
`refresh_recovery_sessions` that declines to offer a segment whose heartbeat is
recent. That is a `v3` format bump and was not done. Recording it because
declining to force a pattern that nearly fitted is the right call and the
reasoning is the valuable part.

The queue-cap principle **did** map: `begin_recovery_segment` leaks the previous
segment file when `write_segment` fails, because the `previous` cursor is taken
before the write and removed only after it. Repeated store failures accumulate
stale segments that are then offered as crashes. Small, real, unfixed.

### Why this file kept describing work that was already done

Four P2/P1 items were found **already fixed** when someone went to work on
them. The cause is not sloppy auditing: it is that a fix can be complete and
invisible at the same time.

`crates/opendoc-import/src/docx/table.rs` — ~520 lines implementing the whole
of P1-4 — was **untracked**. So was `crates/opendoc-spreadsheet/src/validation.rs`,
which enforces the `strict` rule the P2 list says enforces nothing. So was
`crates/opendoc-app/src/editor/clipboard_html.rs`, and so was the **entire
`opendoc-fuzz` crate**, 13 tests of it, which the workspace builds and runs.

An untracked file compiles, tests, and ships in the working tree while being
absent from `git status` summaries, from diffs, and from any review that starts
from tracked files. It is the worst of both worlds: real enough to pass the
gate, invisible enough that a reader concludes the work was never done — and
one `git clean` from being lost outright.

All of them are now **staged**. Two of agent A's new test files are still
untracked only because A is mid-edit; they must be staged when it lands.

**The standing rule this earns:** when a finding says something is missing,
check the working tree and not only the index — `git status --porcelain | grep
'^??'` before concluding anything is absent.

### Two tabs over one browser store, and the report nobody read

Verified before fixing, and the finding understates it. It is not only that
two tabs clobber: each tab's `MirroredVolume` is the store of record and
hydrates **once at boot**, so a second tab never sees the first's writes again
and keeps flushing a stale view indefinitely. The sharp edge is
`RECENT_DOCUMENTS_KEY` — `recent/documents` is a *single key for the whole
origin*, so one tab's recents list replaces the other's outright.

The fix is a **Web Lock** (`opendoc-volume`), taken in `storage::ready` before
the volume is hydrated. The tab holding it owns the object store; a tab that
does not is memory-only and *says so* — which is the shape ADR 0008 §5 already
uses for a runtime with no IndexedDB, rather than a fourth invented mechanism.
Released by the browser when the tab goes away, crash included, which a
heartbeat key inside the contended store could not manage.

Three things that were deliberate rather than obvious:

- **A waiting tab keeps mirroring.** Its queue is exactly what gets written if
  it is promoted, so discarding it would throw away the user's work at the
  moment it became saveable. That queue is unbounded work nobody may ever
  drain, so it is capped (`WAITING_PENDING_BYTES_CAP`) and **hitting the cap is
  reported**, not absorbed.
- **Promotion is real.** A blocking second lock request means closing the
  owning tab hands the store to a waiter, which hydrates under the same rule
  `ready` already uses — a durable value must not overwrite a live one, so this
  tab's own work outranks what the departed tab left.
- **No Web Locks in the runtime is reported too** (jsdom, older browsers).
  Carrying on is still right for a single tab, but a second tab will clobber,
  and that is now said rather than left to be discovered.

`storage_ready`'s report was being thrown away at the `await`. It is now kept
(`invoke.ts::storageReport`) and surfaced — as a **persistent banner, not a
toast**: this state lasts as long as the tab does, so a notice that erases
itself after 2.5 seconds would tell the user once about something still true an
hour later.

`cdp.mjs` gained `newPage()`. `launchChrome` twice gives two *profiles*, which
share no IndexedDB and contend for no lock — so the existing "two browsers"
collaboration checks structurally could not see any of this. The comment in
`e2e.mjs` that recorded the clobber as a known limitation is now false and was
corrected.

**Mutation evidence.** Run against a private `rsync` copy with its own
`CARGO_TARGET_DIR` and its own ports — never the live tree. Removing the
arbitration from `ready()` (1,776 bytes, i.e. exactly the pre-fix function)
fails both new checks:

- *a second tab is told it does not own durable storage* — "two tabs both
  claimed to own durable storage, which is the clobber:
  `{persistent: true, entries: 7}`"
- *a second tab cannot overwrite the first tab's recents list* — "a tab that
  does not own storage still wrote to it, replacing the owner's recents list"

And the §7 pattern once more: under that same mutation the three **existing**
storage checks — including "the recents list survives a page reload" — still
pass. The suite could not see this at all.

e2e is now **64/64**.

## Wave 5 — 2026-09-14

Three agents were terminated mid-flight by a session rate limit, not by any
fault in the work. One of the three (`opendoc-app` correctness) had already
completed and reported; its work was in the tree and nothing was lost. The undo
agent had not started. The **signature agent had got much further than it
looked** — my status check at 22:50 happened to fall in its reading phase, and
it wrote 1,308 lines between 23:01 and 23:21.

### The undo bug is fixed — and the obvious fix would not have worked

Reproduced first: removing the exclusion fails at seed 136 with
`block 3 with w<51>ords` against `block 3 with words`.

**The rule, now in ADR 0017 as *Amendment 2026-09-14: the fold applies the
batch's own discards*:** the fold is a **model of one merge**, so its last state
must be the document the batch lands. Where the merge's answer depends on the
whole operation set, the fold has to be told the whole set's answer — so it
**skips** every operation the batch merge discards. Skipping is not a capture
failure, because the merge discards it too, so the state after skipping *is* the
state the merge produces.

**The obvious alternative is in the ADR as rejected, and the reason is the
interesting part:** merging the prefix `0..k` as a batch instead of folding
incrementally does *not* work — the prefix `[InsertText]` does not contain the
rewrite, so it lands the same wrong state. **Every** definition of "the state
before operation k" that reads only operations `0..k` has this hole, because the
operation that decides the answer comes *after* k. That is what forces the fold
to see the batch's later operations.

One rule now has three callers (merge, `invert_text_operations`, the fold), so
none can drift. The `copying_fold` oracle skips discards using its **own**
reading of ADR 0007, not the crate's function.

**The detail that makes the proof real:** with the existing generator the
offending shape occurred **3 times in 600 seeds**. Removing the exclusion
without also generating the shape would have been a near-vacuous proof. The
generator now produces it deliberately (one batch in four) and both generated
tests assert a floor on how often it occurs.

7 mutations, zero survivors.

`opendoc-app` 329 → **331**, `opendoc-merge` 223 → **224**, workspace
**1,467 passing, 0 failed**, clippy and fmt clean.

### The table-border default: declined by a third agent, and my brief was wrong

**A correction I should record against myself.** I wrote that
"`opendoc-render` already emits the same inline style". It does not — for a cell
that states nothing, `css.rs` returns early and the gridline comes from a
**stylesheet rule**. The conclusion I drew (the screen would look identical)
holds; the mechanism I gave for it was wrong.

And the fact that changes the decision: **`opendoc-layout` already draws the
gridline for an unstated table** — `default_cell_border()` is 15 twips and
`#999999`, pinned by its own test. So screen and paper *already agree with
nothing in the model*. The split is **2 against 2**, and the two that changed in
wave 4 are the exporters, not the model.

Why `TableCell::empty()` is the wrong home whatever the answer: it has four
callers and **three are not "a user made a table"** — `google_import` for cells
it cannot map, `docx_write` as a stand-in for a covered position *while
writing*, and `block.rs` padding a ragged row during repair. A visual default
there makes an imported Google table and a repaired row assert a border their
source never stated — *the exact lie wave 4 removed from `docx_write`, moved one
layer below where any exporter can see it.*

**The real defect is that "absent" is overloaded**: it means "nobody said" for an
OpenDoc table and "the source says none" for an imported one, and nothing can
tell them apart. The clean fix is a **table-level border on `BlockKind::Table`**
— the thing both exporters cite by name as the reason they now write nothing. It
touches six crates. The narrowest interim repair is in `opendoc-import` alone:
have `docx_write`/`odt_write` materialise the same default the other two
surfaces already draw.

### Item 20, the signature scope — proved by exploit, mechanism built, wiring open

Verified after the interruption: it compiles, and `opendoc-format` 44 → **47**,
`opendoc-sign` 14 → **20**, `opendoc-store` 54 → **67**. Workspace **1,464
passing, 0 failing**; clippy `-D warnings` and `fmt --all --check` clean. The
agent never got to run its own gate, so I ran it.

The design, which is better than the one I sketched in the brief:

**A version signature cannot live inside the manifest it covers.** The manifest
is content addressed, so adding a signature would change the hash the signature
names. It is therefore a **sidecar keyed by the manifest hash** — exactly as a
version label and a blob signature already are — stored beside a
`VersionCoverageRecord` that is the signed payload.

The part worth keeping: that record is **derived from the manifest, never
authored**, so *a signer cannot assert coverage the manifest does not have*. It
restates the parent, snapshot, segment and blob hashes so the signed bytes stay
readable after the manifest they describe is gone.

New surface: `sign_version`, `verify_version_signature`,
`verify_version_coverage_with_public_key` in `opendoc-sign`;
`ManifestChainProblem`, `ManifestChainAudit`, `SignedVersion` in
`opendoc-store`.

**ADR 0002 and ADR 0003 are corrected**, and correctly: ADR 0002 now opens with
*"Implementation status corrected 2026-09-13 — several sentences in this ADR
described a manifest signature that did not exist; they are now marked with what
is built, what is wired, and what is neither."* That is the right treatment for
a documented security claim the code did not honour — the pattern that has
already cost this project a wave.

#### It proved the hole by exploit, not by reading

A throwaway test **in the private copy only** ran the attack end to end: save,
edit, save, sign, save; then rewrite the head manifest as
`{ parent: None, operation_segments: [], ..M2 }` — *snapshot untouched* — and
commit it. The test **passed**, meaning the document opens, `list_versions`
drops from 3 entries to **1** (all history gone),
`document_signatures_cover_current_state()` returns `true`,
`verify_current_signatures()` returns `"signed"`, and `warnings` is **empty**.

**A distinction the finding did not know.** Truncation *by deletion* is not
silently survivable: removing an ancestor makes `read_operation_envelopes` fail
and the document will not open at all — an availability refusal, not an
integrity check, and itself the "cannot open" trap P0-5 names. **Truncation by
rewrite is** silently survivable. That is the actual vulnerability.

**One clause of the finding is wrong:** "no blob-content digest in it".
`AppBlobRef::hash` *is* serialised into the signed snapshot, so blob digests
were already covered. What is absent is the manifest, the parent and the segment
list.

#### Does the manifest transitively name everything?

**Yes for history; no for sidecars; never for presence** — established rather
than inherited from my brief.
`every_manifest_field_moves_the_hash_a_version_signature_binds_to` mutates each
of eleven manifest fields and asserts the derived hash moves.

Missed: sidecars keyed by hash (version labels, blob signatures, tombstones,
candidate heads), the mutable branch head, and — the one that bites —
**presence**. A signature is a statement about bytes; delete every ancestor and
the signed manifest is byte-identical and still verifies. No cryptographic
construction fixes that, which is why `opendoc-store` gained
`audit_manifest_chain`.

#### 23 mutations, 23 killed — and one the design could not have caught

**M14 is the one worth recording.** The agent's first design had **no test that
could kill it**: because the payload is derived from the manifest, a wrong
`target` field could never change the verdict. It noticed *while enumerating
mutations*, and added a test that signs correctly and then relabels `target`.
That matters because the store **files sidecars by `target`**. Two other
mutations (M9, M13) were killed by assertions whose messages were useless
(`{:?}`, a bare `unwrap`); it rewrote both and re-ran to confirm they now name
the problem.

It also **removed a `Cycle` variant it had written itself**, because a parent
link is a content hash and `read_manifest` rejects bytes that do not hash to
their name — so a cycle needs a SHA-256 preimage. An untestable branch, deleted
on the grounds that "this project has been bitten by exactly that".

The decisive test is
`a_version_signature_sees_a_rewritten_rootless_manifest_where_a_snapshot_signature_cannot`:
one real two-version chain, **both** signatures taken over it — a
`sign_target`-over-snapshot one, exactly what the app does today, and a
`sign_version` one — then truncated by rewrite. The snapshot signature says
`Signed`; the version signature says `Broken`. The contrast is the point.

#### The ADRs were false, in these words

- ADR 0002: *"…then sign canonical binary manifests that reference those objects
  by hash"* and *"Branch heads are mutable binary pointers to signed manifests."*
  **Nothing signed a manifest.**
- ADR 0002 rationale: *"Signing one canonical manifest avoids signing large
  mutable document blobs."* **Backwards** — the app signs the entire encoded
  snapshot, the precise thing that sentence says it avoids.
- ADR 0002's "Validation Required" clauses had no implementation and no test;
  now marked met **at the crate level, not the application level**, each naming
  its test.
- ADR 0003: *"Signing covers current state plus retained history reachable from
  the manifest."* False as written.

Two further divergences found while auditing and **recorded rather than quietly
fixed**: `AppBlock::id` is serialised into the signed payload with no skip, so
block UUIDs *do* affect document signatures though ADR 0003 says they should
not; and ADR 0003's five signature states have **no slot for "truncated
history"** — `broken` would be the wrong answer, because nothing was altered.

#### What remains, stated plainly

**Fixed 2026-09-14.** The handover landed as its recommended explicit command:
`sign_current_repository_version_with_openssh_private_key` signs the already
committed manifest, rather than retaining a private key for a later save.
`read_signatures` verifies version sidecars and audits the named chain; its
three failure classes are warnings rather than open failures. The snapshot
signing command remains available, but explicitly carries no history claim.

`opendoc-format` 44 → **47**, `opendoc-sign` 14 → **20**, `opendoc-store` 54 →
**67**. Workspace **1,464**, clippy and fmt clean.

## Wave 4 final gate — 2026-09-13

| Gate | Result |
|---|---|
| `cargo test --release --workspace` | **1,442 passed, 0 failed** |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `generate:commands -- --check` | passes |
| `typecheck`, `build:wasm`, `build`, `smoke` | pass |
| `native-check` | **32** |
| `npm run e2e` | **68/68** |

Per crate: app 329, import 224, merge 223, layout 112, service 101, spreadsheet
91, render 74, core 56, store 54, format 44, wasm 32, pdf 31, citations 28,
api 16, sign 14, fuzz 13.

Wave 3 ended at 1,383 and e2e 65. Wave 4 added **59 Rust tests and 3 e2e
checks** — and, more to the point, four corrections to findings I had written
down as fact.

## Wave 4 — the remaining items, 2026-09-13

Four workers, disjoint crates. This wave takes every open item **except** the
two that cannot be owned by one worker.

| Worker | Owns | Items |
|---|---|---|
| 1 | `opendoc-app`, `opendoc-api`, `apps/desktop/src` (minus `src/fonts`), `e2e.mjs` | the `editor.rs` quadratic; `signature-predates-payload-format`; CSV/XLSX → `AppExport`; citation warnings reaching an editing user; find navigation into header/footer; the app stylesheet's `depth-N` cycle |
| 2 | `opendoc-core`, `opendoc-import`, `opendoc-merge` | DOCX table borders (both halves, which is why it needs `opendoc-core`); the `apa-7th` default that now warns on every export |
| 3 | `opendoc-layout`, `apps/desktop/src/fonts` | `U+25E6` missing from the font subset — and, more importantly, proving no other glyph's metrics moved |
| 4 | `opendoc-service` | document-thread reclamation — the last open service item, and the only single-owner item on the whole list |

**Deferred to a serialized wave 5**, because exclusive crate ownership cannot
express them:

- **Item 13, character-granular annotations** — core + merge + app + render +
  api. Five crates, every one of them owned by a worker above.
- **Item 20, signature scope** — app + store + format + sign, plus ADR 0002 and
  0003. It also *interacts* with worker 1's `APP_DOCUMENT_FORMAT` bump: one
  changes what version the signed payload declares, the other changes what the
  payload covers. Doing them concurrently would make neither reviewable.

### What went into every wave 4 brief that was not in wave 3's

Each cost real time or real damage earlier, so each is now stated up front:

- **Delete your private mutation tree and its `CARGO_TARGET_DIR` when done.** The
  private-copy rule is right and is staying, but wave 3's version of it left
  100 GB behind on a host that reached 99% disk.
- **Never use `pkill -f` or `pgrep -f`.** Three incidents, one near-miss on
  another agent's `rsync`, and seven shells that span for hours in a loop that
  could not exit.
- **Release builds only, in the working loop and not just the reported gate.**
  Bare `cargo check`/`test` wrote 39 GB into `target/debug` that nothing here
  consumes.
- **Expect your own brand-new test to survive its own mutation.** Three separate
  agents hit this in wave 3, one immediately after being warned about the exact
  pattern. It is named in every brief now with that example attached.

### `apply_batch` — 2× faster, and a pre-existing undo bug found on the way

**The attribution was right, and the sub-attribution was too.** Re-measured
from scratch: the cost is not the merge. It is **copying and freeing the whole
document once per operation, plus validating it once per operation**. At 1,500
blocks, instrumented: clone 723 ms, the drop of the previous copy ~764 ms,
`Document::validate()` 1,476 ms — and *the actual apply, 45 ms, 1.4%*.

| blocks | before | after |
|---|---|---|
| 200 | 38 ms | 19 ms |
| 800 | 638 ms | 320 ms |
| 1,500 | 2,497 ms | 1,122 ms |
| 3,000 | **10,626 ms** | **5,135 ms** |

Twenty keystrokes at 3,000 blocks: 18.7 ms each → **11.4 ms**. Minimum of three
runs, two private trees, same toolchain, no instrumentation in the timed code —
and it said the machine was noisy and single runs varied up to 2×, so read the
minima.

`merge_operations` is now *literally* `clone + merge_operations_into`, so the
two cannot drift. The scratch is materialised lazily (a one-operation batch
copies the document **zero** extra times), the last fold is skipped because its
inverse was already captured, the fold stops at the first failure, and the
causal context is **carried** rather than rebuilt from a full journal rescan per
operation. It also deleted an `Irreversible` override that had become a second
copy of a rule agreeing with the first by construction — a branch no mutation
could kill.

#### The equivalence proof is the strongest in the project so far

I asked for fixtures captured before the change. It did better: the oracle is
`copying_fold`, **the pre-change fold written out in full inside the test**, and
600 generated batches compare minted operations (id, payload *and causal
context*), every captured inverse, and the landed document.

Then the step that makes it mean something: it **rebuilt the pre-change tree**
from `git show :` of every file it touched and ran that same test against it.
Passing there proves `copying_fold` reproduces the original implementation;
passing here is the equivalence claim. Coverage floors assert the generator
really produced expressible inverses, deferred character ops, `Irreversible`
tails and outright refusals — and the floors are live: the test first failed on
`refusals >= 1`.

13 mutations, all killed. The performance guard uses no wall clock — it asserts
a one-operation gesture copies the document exactly once, and that a batch 8×
as long does not copy 8× as often.

#### A pre-existing undo bug: **undo adds text**

Found by the generated undo test at seed 136, and it **reproduces identically on
the pre-change tree**, so it is not this work's doing:

```
InsertText { inline_id: X, offset: 14, text: "<51>" }
UpdateInlineText { inline_id: X, text: "rewritten 374" }   // same run
```

ADR 0007 says a whole-run write resets the run, so the *batch* merge discards
the `InsertText`. But the scratch fold merges one operation at a time, so each
is alone in its own merge and the reset never applies: the fold applies the
insert, and the `UpdateInlineText` after it captures a "previous value" of a
state **the document was never in**. At undo, the character op correctly inverts
to `Nothing` per ADR 0017, so nothing takes those characters back out — and the
undo *adds* text.

Deliberately not fixed: fixing it moves an inverse, which is exactly what this
task was told not to do quietly, and it needs its own decision — probably an
ADR 0017 amendment about whole-run resets inside a batch. The test names and
excludes precisely that shape, carries the reproducer in a comment, and
**asserts the exclusion is still reachable** so it cannot quietly become dead
weight.

#### What remains, and why it was refused

`Document::validate()` is now **~79% of what is left** (1,083 ms of 1,345 ms at
1,500 blocks). It is load-bearing: its `Err` is the *only* thing that sets
`inverse_capture_ends_after`.

Batching it across a window was **considered and rejected on evidence**:
validity is **not monotonic**. The existing test's own gesture — insert a block
reusing a live inline id, then delete the original — is invalid at step 1 and
valid at step 2, so a window-end check would miss the failure and silently trust
inverses the current code refuses.

The real remedy is outside that agent's territory: `Document::validate` rebuilds
`BTreeSet`s of every block and inline id, with a `StableId` clone each, on every
call — roughly **4.5 M allocations** for a 1,500-block fold. A
`validate_after(&self, changed: &[StableId])`, or simply reusable id sets in
`opendoc-core`, would take the remaining quadratic term out.

`opendoc-app` 326 → **329**.

#### One thing it called pre-existing that was mine

It reported a clippy `field_reassign_with_default` in
`crates/opendoc-citations/src/tests.rs` as pre-existing and outside its
territory. It was correct from its own baseline — but I had introduced it an
hour earlier, fixing the citation tests agent 2 could not reach. Fixed with
struct-update syntax. Worth recording because "pre-existing" is relative to
whenever an agent forked its copy, and in a wave this long that is not the same
as "not ours".

### The PDF draws real table borders — and corrected my brief twice

Both corrections matter more than the fix.

**1. `PaintItem::Stroke` carries no colour at all.** So it was worse than
reported: `opendoc-pdf` never issued an `RG`, and the default grid printed
**black** where the screen draws `#999`. Every interior boundary was also
stroked twice.

**2. "A cell with `BorderStyle::None` still prints a line" is not a bug on a
shared boundary — and a PDF that "fixed" it would have been wrong.** `.doc-table
td` gives every cell `0.75pt solid #999` and the table is
`border-collapse: collapse`, so a `none` border has *used width 0* and loses to
the neighbour's default: **Chrome draws that line too.** Treating `none` as
"skip this edge" would have erased lines the screen draws. It was still a real
bug on the table's outer rim, and for width, colour and style everywhere.

I had written that finding into the brief as flat fact. It was half right, and
the half I got wrong would have produced a confident, tested, wrong fix.

Also corrected: the fields are `border_start`/`border_end`, direction-relative
like CSS logical properties — which is why an RTL table's `border_start` is now
resolved to its *right* edge.

#### What it is now

A new `PaintItem::Edge` — a **segment**, not a rectangle, centred on the
boundary, carrying thickness, colour and an optional dash. `Stroke` stays for
what it actually is: the checkbox and the image frame. A new `borders.rs`
reproduces **CSS 2.1 §17.6.2.1** — used width, then style priority
(`double > solid > dashed > dotted > none`), then document order — and walks each
boundary once. That rule is not a preference: the screen already collapses that
way, and ADR 0014's whole point is that paper and screen cannot disagree.

Three silences it found adjacent and named rather than left:
`pdf-cell-border-width-not-measured` (the box geometry still uses the constant,
so a stated width is drawn right and *measured* wrong),
`pdf-cell-background-not-drawn`, `pdf-cell-padding-not-measured`.

#### The evidence is in pixels

A 3×3 fixture rasterised with `pdftoppm -r 300` and the PPM parsed directly:
2.25pt solid `#cc0000` measured as a 10px unbroken run of exactly `cc0000`;
dashed as 9 runs of 38px with 37px gaps; dotted as 35 runs of 9–10px; 3pt
`double` as two 4px lines with a 4px gap — a 12px band. The all-`none` cell's
rim **stops**, while the boundary it shares with its neighbour is still drawn
grey. Only five non-black colours appear on the page, exactly the five intended.

**34 mutations, 34 caught.** Two rounds found real weaknesses that produced
**source changes, not test tweaks**: a redundant zero-width guard that made the
real one unmutatable (removed), and a leak test that could not see an unbalanced
`q`/`Q`. Three fixtures were strengthened after a mutation survived — the `none`
case had used `CellBorder::none()`, whose width is already 0, so it would have
passed by accident.

`opendoc-layout` 102 → **112**, `opendoc-pdf` 25 → **31**.

#### `#999` was stated in three places; now once

It asked for two substitutions outside its territory and I made them —
`styles.css` and `standalone.rs` now read `var(--doc-cell-border-color)` from
the projected `TypeScale`. **A guard caught me doing it wrong**:
`the_app_stylesheets_fallbacks_agree_with_the_projected_scale` failed with
*"styles.css states --doc-cell-border-color as #999, the type scale projects
#999999"*. Same colour, different string — and the point of the rule is one
spelling. Exactly the check that should exist.

#### The e2e check, and what I did **not** assert

Added *"a stated cell border reaches the screen, including when it is none"* —
e2e is now **68/68**.

The agent proposed asserting that the collapse winner (wider border) shows up in
`getComputedStyle`. I did not, because under `border-collapse: collapse` Chrome
reports each cell's **own** border, not the boundary's winner — so that
assertion would have been reading the input back to itself, which is this file's
entire subject. The check proves what it can: a stated border reaches the screen
with its style, width and colour; `none` reaches as `none` with zero used width;
and an untouched cell still carries the projected grey, so it cannot pass by
borders having stopped working altogether. The collapse rule is proven in Rust
and in the 300dpi raster instead, and the check says so in its comment.

### Wave 4: the `opendoc-app` group — and the quadratic was not the cost

All six items landed. `opendoc-app` 307 → **326**, e2e 65 → **67/67**.

**The headline is a correction to the item I had been carrying since wave 1.**
The `editor.rs` quadratic is real and is fixed — `DocumentIndex` now records a
`BlockPath` and resolves in O(nesting depth), taking block-node visits at 1,500
blocks from **2,251,500 to 3,000**. But the agent instrumented the whole path
afterwards instead of declaring victory, and found it had been chasing the wrong
cost:

| stage, at 1,500 blocks | time |
|---|---|
| pass 1 | 0.21 ms |
| split-apply | 15 ms |
| pass 2 | 0.64 ms |
| planning | 0.39 ms |
| **`apply_batch`** | **2.54 s — 99.2%** |

So a 750× reduction in block visits bought **~17–20% of wall clock**, and the
agent said so plainly rather than quoting the visit count as if it were the
speedup. It also noted its own A/B was polluted by a counter running once per
visit. At 3,000 blocks a select-all Bold still takes about **10 seconds**.

The real cost, now assigned as its own item: `apply_batch` clones the document
once and then, **per operation**, calls `invert_operation` and a full
`merge_operations` — ending in `Document::validate()` — against the growing
scratch, plus a `CausalContext` rebuilt from scratch each step. A 1,500-operation
batch is ~1,500 full merges and validations of a 1,500-block document. The agent
declined to touch it because the per-op scratch fold is **load-bearing for
inverse capture** (ADR 0017), which is the right instinct.

The performance guard it left is the shape to copy: it compares the **growth
ratio** between N=200 and N=800 (≤6× for 4× the document; quadratic is 16×), so
no constant comes from the implementation, with a floor so a dead counter cannot
pass.

#### The signature-format handover was understated

Wave 3 added `skip_serializing_if` to **`AppBlock` as well as
`AppBlockProperties`** — including `level`, `ordered`, `rows`, `table`, which
used to serialize as `null`. So **every** v1 document was re-encoded, not only
formatted ones: a plain paragraph has an unset `level`. Every saved document
reported `broken-document-signature`.

And one layer further down, outside the handover entirely: `decode_segment`
refused any crash-recovery segment whose `base_format` differed from the
constant — so the version bump would have **thrown away unsaved work on the
first restart after an upgrade**, exactly when a journal matters. Earlier
readable formats now replay with `recovery-journal-earlier-base-format`.

The v1 test builds a *genuine* v1 head — snapshot re-stamped, signature re-made
over the v1 signing payload — with a **control** asserting the same restated
head verifies when it declares the current format, so the helper cannot be
signing the wrong bytes.

#### Three things nobody had asked for, found while doing the asked-for work

- **Tab-delimited export was labelled `.csv` / `text/csv`.** It is now `.tsv` /
  `text/tab-separated-values`; the frontend had hardcoded both for either
  delimiter.
- **The injected list-style rules land *after* `styles.css`**, so
  `.doc-body ul.doc-checklist` stopped beating `.doc-body ul.depth-1` on source
  order — a depth-1 checklist would have drawn a bullet *beside its checkbox*.
  Fixed by making the selector one step more specific and therefore
  order-independent. Only observable through Chrome's computed
  `list-style-type`, which is what the new e2e check reads.
- A citation purity test **passed under mutation** in its first version because
  it only asserted absence from source state, not presence in the projection.
  Strengthened. Seventh occurrence of that pattern.

#### My table-border request came back "no", with a better argument than mine

I asked for new tables to carry explicit 0.75pt `#999999` borders so the DOCX
change would not make new tables export lineless. Two of my three points checked
out — 0.75pt is exactly `w:sz` 6 so no approximation warning fires, `#999999` is
exact, and the screen would not change. **The third is why it stopped.**

Cells are created in three places and only some are in `opendoc-app`:
`default_table_block()` and `insert_table_after_sized` (its), but also
`TableCell::empty()`/`TableRow::empty()` in **`opendoc-core`**, and
`TableRow::filling(...)` in **`opendoc-merge`** — which the row merge invents
when the last real row is deleted, and the cell merge adds on a concurrent
column insert.

Borders applied only at the insert paths give a table whose lines **stop** at
any cell another replica's column insert added. That surprise appears during
ordinary concurrent editing, not merely on export, and is worse than the one it
fixes. **The default belongs in `TableCell::empty()`/`TableRow::empty()`, or
nowhere.** Deferred until the PDF border work lands, so the whole chain — screen,
PDF, DOCX, ODT — can be verified to agree at once.

It also declined the e2e check I offered to take: the browser has **no drivable
path for a `.docx` import** (`openFile` opens a real picker; the existing image
test uses a drop event the import menu does not accept), and the exported zip's
`word/document.xml` is deflated, so asserting on it would need central-directory
parsing in the harness. Three lines in `docx_write_tests.rs` instead, where the
package reader already exists. Right call.

### Wave 4: DOCX borders — and a design argument I asked for that came back "no"

I briefed this as *"the coherent fix needs a table-level border property in
`opendoc-core`"*. The agent **declined, and was right**, for four reasons I had
not weighed:

1. **It cannot be done in one wave.** `BlockKind::Table { columns, rows }` is
   destructured or constructed exhaustively — no `..` — at ~15 non-test sites
   across `opendoc-app`, `opendoc-render` and `opendoc-layout`, all off limits.
   Adding a field breaks compilation it cannot repair.
2. **The design already exists one field over.** `docx/table.rs` already
   resolves the *other* table-level OOXML default, `w:tblCellMar`, onto each
   cell, with a comment saying exactly why. `w:tblBorders` is the same shape.
3. **Nothing would draw it.** `opendoc-render` emits per-cell inline styles and
   `opendoc-layout` strokes a constant. The field would be the half-feature the
   brief itself warned against.
4. **Real producers agree with the per-cell reading.** LibreOffice *never*
   writes `w:tblBorders` — checked, by having it convert a bordered HTML table;
   it resolves to `w:tcBorders` on every cell. `w:tblBorders` is Word's
   spelling, mostly via table styles.

A table-level field is still the better end state — it would preserve the shape
of a `w:tblBorders` and give `insideH`/`insideV` a home — but it needs a
coordinated wave over five crates. **Recorded, not done.**

#### What the fix actually is

Three layers resolved innermost-last, as WordprocessingML does: the `w:tblStyle`
the table names **and its whole `w:basedOn` chain**, then the table's own
`w:tblBorders`, then the cell's `w:tcBorders`. Table styles were **not parsed at
all** before, so `TableGrid` — the commonest bordered table in the world —
imported borderless.

Which of six edges a cell inherits is decided by its **rectangle**, not its
origin: a cell spanning to the last column takes the table's right edge, one
spanning to the last row takes its bottom, interior cells take
`insideH`/`insideV`.

Two new named ADR 0010 warnings: `docx-dropped-table-border` (the diagonals,
the only edges with no per-cell equivalent) and
`docx-dropped-table-style-banding`.

The writer's invented `single sz=4 color=auto` grid is gone, and **the ODT
writer had the same defect the brief never mentioned** — a
`DEFAULT_CELL_BORDER = "0.5pt solid #000000"` on every unstated edge. Also gone.

Verified through a foreign reader: export the LibreOffice fixture, have
`soffice` convert it back, and `diff` the two ODTs' tables. Before: every
`fo:border="none"` became `0.5pt solid #000000`. After: **empty diff**.

#### The product decision, stated plainly because it is user-visible

**An absent border property now means no border**, in the model as in
WordprocessingML and ODF. The editor's `.doc-table td { border: 0.75pt solid #999 }`
is a *gridline* — a view default, like the ones Word draws on a borderless
table — not a property of the document.

Consequence: **a table created in OpenDoc, on which nobody has set a border,
now exports to `.docx`/`.odt` with no lines.** It previously exported 0.5pt
black ones matching neither the screen (0.75pt `#999`) nor anything the document
said. Not a correctness regression, but a bad surprise. The remedy is one edit
in `opendoc-app`'s `insert_table_after` — give new cells explicit 0.75pt
`#999999` borders, and then screen, PDF, DOCX and ODT all agree with nothing
invented in any writer. Handed to the agent that owns that crate.

**18 mutations, 18 killed.** Two of its own new tests survived their first
mutation and were strengthened rather than excused: one merged-cell fixture had
no span reaching the *last* column, so a reader ignoring `grid_span` agreed with
it; one table-style fixture had no edge where `TableGrid` and `Normal Table`
disagreed, so chain order was unobservable. That is the fifth and sixth
occurrence of this pattern in the project.

M18 is the one worth keeping: it restores the design that was rejected (absent
grid ⇒ OpenDoc's own hairline) and the LibreOffice fixture kills it. **The
decision is pinned by a test, not only by a comment.**

`opendoc-import` 212 → **224**.

#### The citation default, and the two tests it broke in a crate nobody owned

`Document::new` now defaults to `"apa"` — a real bundled CSL style — instead of
`"apa-7th"`, which `resolve_style_name` has never resolved and which wave 3 made
non-exempt from the not-bundled warning. So every new document was tripping a
citation warning on export.

"Roughly 6 merge fixtures" was **exactly 6 tests, 9 assertions**, and every one
moved to values fixed by an external specification — `(Doe 2020)` → `(Doe, 2020)`,
`page 42` → `p. 42` — rather than to whatever made them pass.

It could not fix the two `opendoc-citations` tests its change broke, so it
verified the fixes in its private copy and reported them. I applied them, and
made one of them better than reported: the bibliography test is about *deleted
references being left out*, and its rendered text was only how that is observed —
so I **pinned** the style explicitly rather than re-baselining it to track
whatever the default happens to be next time. `opendoc-citations` **28/28**.

**Still open from this:** `apa-7th` is not an alias for `apa`, so *existing*
documents that stored it keep warning.

### New, and now urgent: the PDF ignores cell borders entirely

`opendoc-layout/src/lib.rs` emits `Local::Stroke { line: self.scale.cell_border }`
— **a constant** — for every cell, never reading `TableCellProperties`. A cell
with `BorderStyle::None` still prints a line; a 2.25pt red border prints as
0.75pt grey. `Local::Stroke` has no colour field at all, so the paint model
cannot express it.

This predates the DOCX work but that work makes it bite, because imported tables
now genuinely carry colour, thickness, style and explicit *none*. The screen is
already right (`opendoc-render`'s `css.rs` emits per-cell `border-*`), so this
is a **paper-only divergence — the second one found today**, after the bullet.
Assigned.

### Wave 4: the service reclaims threads — without trading away the safety property

P2 item 18's last half is closed, and the reason it had been left open was
respected rather than argued around. The previous agent's objection — that the
registry hands out handle clones, so dropping an entry while a clone lives would
let **a second thread spawn for one branch head** — was correct. The fix makes
that condition *answerable* instead of guessing at it.

`DocumentHandle` became one `Arc<HandleInner>` shared by every clone, and the
registry asks `is_solely_held()` **while holding its own lock**. That is the
whole trick: the only way to obtain a clone is through a call that needs the
same lock, so while it is held the count can fall but never rise. A document is
reclaimed only when no clone exists **and** it has been idle for a window.

Three decisions worth recording:

- **No bare `join()`.** If the registry were ever wrong about "last handle", a
  join would block forever *while holding the registry lock*. Instead there is a
  5 s deadline, and a thread that will not stop keeps its uuid **reserved** in a
  `retiring` map that refuses to open the document at all. Where it cannot be
  certain, it refuses rather than serving one document twice.
- **A sweep runs unconditionally when at the limit** — "the difference between a
  cap that lifts and a fuse."
- **`forget_running_documents` was not merely dead, it was unsafe**: it cleared
  the map while threads were running. Replaced by `stop_idle_documents()`, which
  stops only what it can stop safely.

The refusal message now says *which* wait an operator faces — how many documents
are held by a live connection versus merely warm — instead of implying a
restart is the only remedy.

**The safety test is the good one.** It does not count registry entries, which
would not see a second thread at all. It holds a handle, submits alice's edit,
sweeps far past the window, then submits **bob's edit through a handle obtained
after the sweep, observing alice's operation**, and pins the exact merged text.
A second thread would have loaded the log before alice's commit, refused bob's
operation as observing something it had never seen, and lost the CAS on its own
commit.

**9 mutations, 9 killed** — including one that restores the pre-change behaviour
(nothing ever reclaimed) and fails five tests. **M6 survived its first version**:
the limit test used 1 held and 1 warm, so a mutation swapping the two counts in
the refusal message read identically. Fixed with 3 documents. That is the
"passing for the wrong reason" pattern for the fourth time this project, caught
by the agent itself.

`opendoc-service` 94 → **101**.

**Still true, and now said out loud rather than implied:** a document with a
live connection is never reclaimed at any idle setting. A process whose 1,024
documents all have connections still refuses new ones. That is the safety
property, not a gap — and the refusal now explains it.

#### A cross-agent drift, handled correctly

`opendoc-core`'s default citation style changed from `apa-7th` to `apa`
mid-wave, which made `opendoc-service/wire/protocol-frames.json` stale — the
fixture embeds a base document. The service agent regenerated it *after*
verifying in a private tree that `opendoc-wasm`'s `collab_tests`, which
`include_str!` the same file, still passed. I re-checked both consumers against
the live tree afterwards: **service 101, wasm 32**. This is the first time a
cross-crate fixture dependency has been caught and handled without anyone's
suite going red for an unrelated reason.

### Wave 4: the font subset — and a correction to why it mattered

**The item is fixed, but the reason I gave for it was wrong.** I recorded the
depth-1 bullet as a *screen versus paper* divergence. Measured in Chrome, it was
never a screen problem: a `disc`/`circle`/`square` marker box is **2617 units in
all three cases**, identical under the old and new WOFF2, while the face's own
`U+25E6` advance is 1229. Chrome does not size a list marker from the document
face's glyph at all. The screen always drew a hollow circle at depth 1; **only
the paper was wrong**, and only the PDF changes.

The WOFF2 still had to be regenerated — not because the browser needed the
glyph, but because ADR 0014 forbids the two encodings coming out of different
runs.

**Why `U+25A0` was in the subset and `U+25E6` was not: no constraint, just an
incomplete list.** `U+25A0,U+25CF,U+2610,U+2611` was a hand-picked "symbols a
word processor emits" set written *before* `lists.rs` existed and stated the
disc/circle/square cycle. The one place a real limit could have bitten was
checked and ruled out — `opendoc-pdf` embeds every face as Type0/Identity-H, so
the 256-glyph simple-font ceiling does not apply.

**The methodology is the part worth keeping.** Before changing anything, the
agent regenerated with `UNICODES` *unchanged* and confirmed all ten files came
back **byte-identical** to the committed ones. That makes every subsequent byte
difference attributable to the added code point and nothing else — a control I
would not have thought to ask for, and it is what makes the rest of the evidence
mean anything.

Then three independent measurements that no other glyph moved:

1. Through **`ttf-parser`, the reader `opendoc-layout` actually uses** — every
   scalar and the advance of every covered code point, all five faces. Complete
   diff: **one added line**.
2. Through **fontTools at table level** — `hmtx` identical for all 436/423
   previously-covered code points. ~70 apparent outline differences per face
   turned out to be composite accents whose *component glyph id* shifted by one
   because `uni25E6` was inserted into the glyph order; decomposed, **zero**
   differences.
3. In **real Chrome**, per face, every previously-covered code point: *0
   advances moved*. The same run showed the old mono subset genuinely lacked the
   glyph in the browser — `U+25E6` measured 726 (a non-monospaced width, i.e. a
   host fallback) and now measures the bundled 1229.

The agreement suite was re-run against the new fonts with no code change:
**100/100 still pass, and nothing was re-baselined.**

**`Engine::bullet_glyph` — the coverage fallback — is deleted** rather than
fixed, on the grounds that unreachable code which silently substitutes a wrong
shape *is* the divergence rather than a guard against it. The guard moved into a
test that fails loudly.

`opendoc-layout` 100 → **102**. One mutation worth quoting: making
`Fonts::covers` return `true` unconditionally — which would make any coverage
test vacuous — fails **four pre-existing tests**, so the new test cannot be
satisfied vacuously without the suite going red.

**An honest limit the agent stated itself:** the painted-marker test asserts the
*character*, not that the font carries the glyph, so with the subset broken it
still passes. Neither new test covers the bug alone; together they do.

#### Stale claims it found in files it did not own — verified and fixed by me

I re-measured rather than copying, and one of my own readings was wrong first
(I compared KiB against the ADR's decimal kB).

- `opendoc-pdf/src/font.rs` said the subset carries **452** glyphs; it is now
  **453** (sans 453/437 cmap, mono 435/424).
- ADR 0014 §1 said **148 KB** TrueType / **71.7 KB** WOFF2 / 13.9–14.9 per face;
  now **149.1 kB** / **72.0 kB** / 14.0–14.9.
- ADR 0014 §2 said `generate.py` "drops `GSUB`, `GPOS` and `kern`". Checked:
  `kern` is genuinely absent, but `GSUB`, `GPOS` **and `GDEF`** survive as
  shells — with zero lookups and zero features, so the guarantee holds and only
  the description was wrong. Reworded to say what is actually true.

## Wave 3 final gate — 2026-09-13

Everything, after all seven workers landed:

| Gate | Result |
|---|---|
| `cargo test --release --workspace` | **1,383 passed, 0 failed** |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `generate:commands -- --check` | passes |
| `typecheck`, `build:wasm`, `build`, `smoke` | pass |
| `native-check` | **32** |
| `npm run e2e` | **65/65** |

Per crate: app 307, merge 223, import 212, spreadsheet 91, service 94, core 56,
store 54, format 44, layout 100, render 74, pdf 25, wasm 32, api 16, sign 14,
fuzz 13, citations 28.

### Still open

Closed since wave 1: P1-1, P1-2, P1-3, P1-4, P1-8, P1-9, P1-10, and the
ADR 0014 half of §7. P1-3, P1-4 and P1-10 were all found **already fixed** when
someone went to work on them — see the caution under item 2.

Still open, in the order I would take them:

1. **§7's remainder.** The convergence suite is still largely tautological —
   `merge_operations` discards stream grouping before any semantics run, so the
   suite cannot see the property it claims to test. Canonical CBOR is still
   never checked for canonicality. Both are unaddressed.
2. **The cannot-fail assertions the §7 agent tabulated in crates it did not
   own**: `opendoc-layout`'s `bold.lines >= regular.lines` (fixed in wave 2),
   `opendoc-pdf`'s literal `f(x) == f(x)` (fixed in wave 3), `opendoc-store`'s
   `version_label_path` compared against itself (**fixed** — both sites now
   compare against hand-written literals with explicit §7 notes, verified
   2026-09-14). This sub-item is closed.

   *Caution learned the hard way:* this list came from wave-1 agent reports and
   has since gone partly stale — P1-3, P1-4, P1-10 and the `ieee` double-listing
   were all found already fixed when checked. Verify before assigning.
3. **P1-5 / P1-6 / P1-7** version restore, crash recovery and the two-runtime
   recovery store.
4. **P1-11 is closed.** The code was already right; wave 3 supplied the proof
   that was missing and corrected the finding's proposed defence.
5. **DOCX table borders** — the writer invents a border grid, the reader drops
   `w:tblBorders` silently, and the two cannot be fixed independently. Needs a
   table-level border property in `opendoc-core`.
   **Closed 2026-09-15:** `TableProperties::border` now distinguishes document
   silence from explicit borderlessness, and reaches merge, UI, DOCX, ODT,
   OpenDoc JSON, render and PDF.
6. ~~**Table pagination / repeated headers.**~~ **FIXED 2026-09-15.**
   `opendoc-layout` measures a table once (and keeps that unit cacheable),
   then expands it to row flow fragments before pagination. Leading header
   rows are retained as one atomic fragment and painted again before each
   continuing body row; the PDF consumes those page-local paint items from
   the same layout pass. The public editor DTO still exposes one placement per
   logical table block. Layout and PDF regressions prove the header appears on
   both pages, each body row appears once, and no obsolete warning remains.
   **Also fixed 2026-09-15:** merged-cell geometry is now one anchor-owned
   rectangle in Rust layout/PDF. Covered cells do not measure or paint; column
   spans use their combined width, row spans distribute any required height
   across their range and form an unbreakable flow group, and the collapsed
   grid removes interior boundaries. The stale PDF warning was removed only
   after layout and PDF regressions proved it.
7. **`signature-predates-payload-format`** — a signature broken by *our* change
   to the payload encoding currently reports as `broken-document-signature`,
   which means "someone altered this document". Bump `APP_DOCUMENT_FORMAT` to
   v2, accept v1 on read, and emit a distinct code. Wants its own v1-read test.
8. **The `editor.rs` quadratic**, at `:1390` and `:1485` — recorded in wave 1
   and never assigned through three waves. Single owner, `opendoc-app`.
9. **`accept_suggestion` / `reject_suggestion`** still take a caller-asserted
   identity into signed state. Needs a service-side counterpart first.
10. **The app stylesheet** is the last hand-written copy of the `depth-N` list
   cycle; wiring `list_style_type_rules` through needs a field on
   `AppDocumentLayout`.
10. ~~`U+25E6` missing from the font subset~~ — **fixed in wave 4**, and the
   framing was wrong: it was a paper-only divergence, never a screen one.
   Outstanding from it: an e2e check that the bullet glyphs come from the
   bundled faces. The decisive form needs CDP `CSS.getPlatformFontsForNode`
   (assert exactly one entry, `OpenDoc Sans`, `glyphCount: 3` for `•◦■`) —
   with the glyph missing it returns *two* entries whatever the host fallback
   is, where a width comparison would not catch it on a host whose fallback is
   Liberation Sans, which this one is. Needs a raw `send` exposed on
   `cdp.mjs`'s page object. Coordinator to add once `e2e.mjs` is free.
6. **Reported but unfixed, small:** CSV/XLSX export returns `String` where it
   should return `AppExport` (3 files, two crates); `CitationDatabase::default()`
   hard-codes `style = "apa"` (needs 6 merge fixtures re-baselined);
   `normalize_citation_style` accepts unknown names; `citation_support_warnings`
   never reaches `projection_service`; `actions.ts` hard-codes the style list
   where a `list_citation_styles` command belongs; outbound clipboard HTML in
   `opendoc-render`; `Comment::author` is client-supplied into signed state;
   find/replace cannot navigate to a match in a header or footer.
7. **P2** in full, including the one the layout agent re-confirmed from the
   inside: **ordered-list numbering is implemented twice** and the two
   implementations disagree — the renderer keys counters by `(list_id, level)`,
   the layout by depth, and both count bullets into the ordered counter, so
   bullets→ordered starts at 3. Layout deliberately reproduces the renderer so
   the PDF matches the screen; the duplication is the actual defect.

### Known divergences that are now *warned* rather than silent

- The PDF may carry **one more page than Chrome draws** when a document has
  footnotes, because the editing surface has no page for them. The export warns
  by name (`pdf-footnotes-after-the-body`) and the e2e check allows
  `sheets` or `sheets + 1`. Putting footnotes into the page flow is a frontend
  change nobody has taken on.
- An unparseable colour is reported as `pdf-estimated-unmeasurable-mark` rather
  than silently drawn black.
- `.notdef` is no longer recorded in `ToUnicode`. It was one glyph for every
  uncovered character, so an Arabic paragraph extracted as a single letter
  repeated. Absent is honest; wrong is not.
- Chrome's 1/64px quantisation of a non-integral `line-height` is modelled, but
  milli-twips cannot represent 1/64px exactly, so the conversion back costs
  ≤1/64px once per fragment. It does not accumulate across lines.
