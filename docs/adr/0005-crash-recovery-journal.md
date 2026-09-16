# ADR 0005: Crash Recovery Journal and the Unsaved-Work Guard

Status: accepted. Amended 2026-09-12: the segment header carries the two
sequence watermarks. Amended 2026-09-13: the header also declares its base
snapshot's format and carries the unsaved signatures, and a failed journal
write heals itself; the format string is `opendoc.recovery-segment.v2`. See the
amendments at the end.

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
- **One session at a time.** Two concurrent windows over the same store each
  offer the other's live segment as a crash, and discarding one deletes a file
  the other is appending to. The appending window now heals itself rather than
  losing crash protection for the rest of the session — see the amendment
  below, which also says why ADR 0008 §6's Web Lock does not transfer here.
  OpenDoc is single-window; revisit with F2/F3.

## Amendment: the header carries the numbering watermarks

Dated 2026-09-12, with the separation of envelope identity from operation
identity (`docs/adr/0015`, amendment).

The invariant in §2 — the segment replays to exactly the current in-memory state
— covers the numbering counters, because a replica that resumed below one would
re-mint an identity the repository already holds. They cannot be reconstructed
from the segment's frames: the frames begin at the base snapshot, and everything
issued before it is behind that snapshot rather than in front of it.

`RecoverySegmentHeader` therefore carries `next_envelope_seq` and
`next_operation_seq` as they stood when the base snapshot was taken, and the
replay takes the larger of the header's watermark and anything the appended
frames show. The header's `format` is now `opendoc.recovery-segment.v1`. The
frame layout did not change, so the file magic did not either.

A `v0` segment is still read and is not guessed at: in `v0` a single counter
numbered envelopes and operations alike, so that actor's highest envelope number
is exactly what both counters stood at. The replay reports
`recovery-journal-legacy-numbering` naming the segment, because a file written
by an older build is a fact the user is entitled to see rather than something to
read silently. Any other format string is refused.

## Amendment: the header declares its base format and carries the signatures

Dated 2026-09-13, with the crash-recovery data-integrity pass.

Two things a `v1` header could not say about itself, both of which cost the
user work:

**The base snapshot's format.** §4 made `RecoverySegmentHeader` embed a whole
`AppDocument` as `base` while the file declared only *its own* format, so one
`recovery-segment.v1` file could carry either `app-document` shape and nothing
inside the segment could tell them apart. The next `AppDocument` bump would
have decoded into whichever fields happened to line up and replayed a document
nobody wrote. The header now carries `base_format` — the same string a
repository snapshot declares — and a segment naming a format this build cannot
replay is refused by name. A `v0`/`v1` segment carries no declaration; it is
still replayed, and says so with `recovery-journal-undeclared-base-format`,
because an assumption the reader cannot check is a fact the user is entitled to
see.

**The unsaved signatures.** Signing is not a typed operation and it makes the
document dirty, so a segment exists *precisely* when a just-minted signature
has not reached the repository. The header had nowhere to carry one, so
`recover_session` could only `self.signatures.clear()`: signing, crashing and
recovering returned an `unsigned` document with an empty warning list and
nothing anywhere to say a signature had been discarded. The header now carries
`signatures`, the replay restores them, and — because a recovered signature
covers the base snapshot — a replay that moved past it reports
`recovery-journal-broken-signature` rather than presenting a signature as
covering state it does not.

The *offer* had the same blind spot from the other side. `AppRecoverySession`
counts operations, and signing is not one, so a crash that caught a signature
and nothing else was offered as a session with zero changes and an empty
operation list — and the user was asked whether to discard "nothing" when a
signature was what discarding destroys. `refresh_recovery_sessions` now emits
`recovery-journal-unsaved-signature` naming the session and the count. The
warning belongs to the set of segments currently *on offer* rather than
accumulating: refreshing rebuilds it, so recovering or discarding a session
takes its warning with it. The offer DTO would be the better home for this, but
its TypeScript shape is a hand-written literal in the contract generator rather
than a projection of the Rust struct, so a field added on the Rust side would
not reach the client and no gate would notice; see the report accompanying this
change.

The header's `format` is therefore `opendoc.recovery-segment.v2`. The frame
layout did not change, so the file magic did not either, and `v1` and `v0`
segments are still read.

### The "one session at a time" limitation is not permission to fail permanently

The limitation above says two concurrent windows over one store would each
offer the other's live segment as a crash. What it did *not* say, and what was
true, is what happened next: if the second window discarded the first's live
segment, the first's `append_segment` failed for ever, `sync_recovery_journal`
never reset `self.recovery.cursor`, and `push_model_warning` deduped the
warning after the first one. Crash protection was over for the rest of the
session, silently.

`sync_recovery_journal` now drops the cursor whenever a sync fails, so the next
dispatched command re-snapshots under a fresh session id and journalling heals
itself. This is only the difference between a limitation and a permanent,
non-self-healing failure; ownership remains unsolved for the native shell.

The browser half of the same theme *is* solved, and differently: ADR 0008 §6
gives one tab the `opendoc-volume` Web Lock and makes every other tab
memory-only and say so. That shape does not transfer to the native journal as
it stands, and the difference is worth stating rather than papering over. Two
tabs contend for *one* set of keys — the volume — so exactly one writer is the
correct answer. Two native runtimes write *disjoint* segment files, one per
session id, and each is a legitimate editor with its own unsaved work that
deserves its own journal; making the second memory-only would lose work rather
than protect it. What collides is not the writes but the *offer*: a segment
whose owner is still alive is indistinguishable from one whose owner died, so
window B is offered window A's live journal and may delete it.

Closing that needs liveness in the segment, not a lock over the store — a
heartbeat frame appended alongside the operation frames, and a
`refresh_recovery_sessions` that declines to offer a segment whose heartbeat is
younger than the crash-detection threshold. That is a format bump (`v3`) and is
not done. Leader election (PLAN77 F2) remains the general answer.
