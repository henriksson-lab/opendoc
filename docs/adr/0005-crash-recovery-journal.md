# ADR 0005: Crash Recovery Journal and the Unsaved-Work Guard

Status: accepted.

Supersedes nothing. Implements FS-6 and FS-7 of
`docs/GOOGLE_DOCS_PARITY_TODO.md` (PLAN77 phase A6).

## Context

Two related ways to lose a user's work existed.

**FS-6.** Only two paths asked before throwing away unsaved changes: the
`new-document` action and the Tauri window-close handler. `open-repository`,
`open-recent`, `import-word` and `import-json` replaced the open document
silently. The checks that did exist were written at the call site in
`main.ts`, so each new document-replacing action had to remember to add one —
four already had not.

**FS-7.** `OpenDocApp` mints a typed operation envelope for every committed
operation, but that journal only becomes durable when a save writes it into
the repository as an operation segment. Between saves it is memory. `kill -9`,
a WebView crash or a panic lost everything since the last commit. Autosave did
not close the gap: it requires `repository_root`, so a document that has never
been saved was never autosaved at all.

## Decision

### 1. The unsaved-work guard lives in the dispatcher, not in the UI

`OpenDocCommand::replaces_open_document` classifies every command with an
**exhaustive match and no catch-all arm**. `OpenDocApp::dispatch_command`
refuses a replacing command while `OpenDocApp::has_unsaved_changes()` is true,
returning `AppApiError::UnsavedChanges`. A caller that has asked the user and
been told to go ahead repeats the dispatch with the top-level argument
`discardUnsavedChanges: true`.

Consequences, in the order they matter:

- Every runtime reaches the app through `dispatch_command`, so no UI, and no
  later service transport, can route around the check.
- The classification cannot be silently omitted: adding a command fails to
  compile until its author says which side of the line it is on.
- Forgetting now fails **closed**. A new action whose author does not handle
  `UnsavedChanges` gets an action that refuses to run, not a document that
  silently disappears.
- The frontend keeps no dirty flag of its own. `has_unsaved_changes()` is the
  single formula (`AppProjectionService::has_pending_save_changes`), and the
  document projection field, the autosave loop, the window-close guard and
  this guard all read it.

`discardUnsavedChanges` is deliberately not part of any command's declared
argument list. It is a dispatcher-level policy acknowledgement that applies
uniformly to every replacing command, and putting it in ~15 argument DTOs
would imply each command interprets it.

### 2. Recovery is a base snapshot plus typed operation envelopes

While — and only while — the open document differs from the repository, a
*recovery segment* exists on disk holding:

1. a header with the document uuid, title, actor, repository binding, base
   manifest, and a **base snapshot** of source state (`AppDocument`, the same
   record type a repository snapshot stores); and
2. every operation envelope committed after that snapshot.

A segment cannot start from "empty": creating, importing and opening a
document are not typed operations, so without a base snapshot a replay could
not reproduce an imported or never-saved document. The snapshot is written
once per segment, and operations are appended one frame at a time, which is
what makes per-keystroke journalling cheap.

The segment is maintained by one call, from `dispatch_command`, after any
command that succeeded. Driving it from the dispatcher rather than from the
places that mint envelopes makes it total: undo, redo and candidate merges
move the journal without minting anything. The rules are:

- clean or closed document → remove the segment (there is nothing to recover
  when the repository already holds everything);
- journal still an extension of what was written → append the new tail;
- anything else (undo, redo, a different document, a new signature) →
  re-snapshot under a fresh segment and drop the old one.

The invariant is that the segment replays to exactly the current in-memory
state, so recovery never resurrects work the user undid.

### 3. Replay reuses the repository's own machinery and commits nothing

`recover_session` replays through `merge_operations`,
`merge_spreadsheet_envelope_streams` and `merge_blob_envelopes` — the same
typed paths a candidate-head merge uses — and writes the result into memory
only. The recovered document is an ordinary unsaved document bound to the same
repository at the same base manifest. Nothing touches the manifest chain until
the user saves, and the save that follows is a normal commit, so the recovered
state validates and signs exactly like any other edit.

Startup never replays. `install_recovery_journal` reports what it found in
`AppDocument.recovery_sessions`, including the operation summaries a replay
would apply, and the user chooses `recover_session` or
`discard_recovery_session`.

### 4. Durable format

`<session id>.recovery`, where a session id is `StableId`-shaped
(`[A-Za-z0-9_-]`, validated before it is ever joined to a path):

```
"opendoc-recovery-v0\n"
u32le length | canonical-CBOR RecoverySegmentHeader
u32le length | canonical-CBOR AppOperationEnvelope
...
```

Canonical CBOR because the envelope encoding is already the repository's, is
already round-trip tested, and gives recovery the same bytes the commit path
would have written. Length-prefixed frames because a crash can land mid-write:
a short tail frame is detected, dropped, and reported to the user as
`recovery-journal-truncated` rather than failing the whole segment.

### 5. `fsync` is deliberately not called

Appends are written and flushed, not `fsync`ed. The threat model is a process
that dies — `kill -9`, a panic, a WebView crash — and the kernel keeps written
bytes across that. Surviving a power cut or a kernel panic would mean an
`fsync` per keystroke-sized gesture. If OpenDoc later wants that, it belongs
behind a preference, not as the default in a text editor.

### 6. Scope: Rust-defined storage trait, filesystem adapter only

`RecoveryJournalStore` is a byte-level, document-type-free trait
(`write_segment` / `append_segment` / `list_segments` / `read_segment` /
`remove_segment`). `FileRecoveryJournalStore` implements it for the Tauri
shell, which installs it over the app data directory.

**The browser/WASM runtime installs no store today, so it is not crash
protected.** This is not an accident of the Tauri code: journalling is inert
without a store, and an IndexedDB adapter implementing the same five methods
is the whole remaining work. That belongs with PLAN77 F3 (browser storage
adapter), which has to solve durable local storage for repositories anyway.

## Known limitations

- **Attachment bytes are not recovered.** A recovery segment carries blob
  *refs*, not blob *contents*; bytes added since the base snapshot live only
  in memory. Replay emits `recovery-journal-blob-bytes` naming the problem so
  the user can re-attach. Storing them would mean a content-addressed sidecar
  next to the segment; deferred, not forgotten.
- **Pre-segment history is truncated.** The recovered journal holds only the
  segment's operations. The repository still holds the earlier segments and
  the chain is written correctly on the next save, but the in-memory audit
  list starts at the base snapshot.
- **One session at a time.** Two concurrent windows over the same store would
  each offer the other's live segment as a crash. OpenDoc is single-window;
  revisit with F2/F3.
