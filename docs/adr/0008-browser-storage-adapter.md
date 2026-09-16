# ADR 0008: Browser Storage — Asynchronous IndexedDB Behind a Synchronous Store

Status: accepted.

Supersedes nothing. Implements PLAN77 F3 and closes the gap ADR 0005 §6 left
open ("the browser/WASM runtime installs no store today, so it is not crash
protected").

## Context

The browser build held everything in memory. Closing the tab lost the document,
`save_local_repository` built a `LocalObjectStore` over a filesystem that does
not exist in WebAssembly and failed every call, and the crash-recovery journal
was inert because journalling does nothing without a `RecoveryJournalStore`.

The obstacle is not IndexedDB's API surface. It is that **IndexedDB is
asynchronous and `OpenDocApp::dispatch_command` is synchronous**, from the
command registry down through `Repository` to `ObjectStore::put_named`. Neither
side can simply change:

- `ObjectStore` and `RecoveryJournalStore` are synchronous by design; making
  them `async` would infect every command, every service and both other
  runtimes to serve one runtime's storage device.
- A browser's main thread cannot block on a transaction. There is no
  synchronous IndexedDB, `Atomics.wait` is unavailable on the main thread, and
  a spin loop deadlocks the page: the transaction's own callbacks need the
  thread you are holding.

So the two cannot be made to meet in the middle. One of them has to stop being
on the critical path.

## Decision

### 1. The in-memory volume is the store of record; IndexedDB is a mirror

`opendoc-store::MirroredVolume` is a flat `key -> bytes` map held in memory.
Every read and write the app performs is answered from it, synchronously, with
no I/O. `MirroredObjectStore` is an `ObjectStore` view of a subtree of it, so
the repository, manifests, heads, lookup indexes and tombstones work exactly as
they do on a filesystem — it passes `verify_object_store_contract` unchanged.

Every mutation is *also* appended to an ordered, monotonically sequenced
pending list. A runtime driver (`opendoc-wasm/src/storage.rs`) drains that list
asynchronously into IndexedDB and calls `MirroredVolume::acknowledge(seq)` when
a transaction commits.

Durability is therefore an explicit watermark, not an assumption:
`sequence()` is what the core has written, `durable_seq()` is what IndexedDB
has committed, and the two differ only while a flush is in flight.

**Alternatives rejected.**

- *Make the persistence boundary async.* The honest version of this is an async
  `ObjectStore`, which means an async `Repository`, an async command dispatch,
  and `async` colouring across `opendoc-app`. It buys nothing for the two
  runtimes that have synchronous storage, and it does not even remove the
  problem: a command would still have to decide what to do while a write is in
  flight.
- *Scope the adapter to explicit save/load points.* Tempting, because a save
  already is such a point — but the recovery journal is the opposite of a save
  point. It writes per gesture, precisely so that work between saves is not
  lost, and that is the half of F3 that matters most to a browser user.
- *Fake synchrony.* Not done, and not doable: see above.

### 2. Content addressing makes write-behind safe; the head is the exception

Objects are immutable and named by their own digest, so a late write can only
ever mean an object is *missing*, never that it is *wrong*. Branch heads are
mutable, and a head that becomes durable before the objects it names is a
dangling pointer — a lost document.

Two properties rule that out, and both are testable:

1. `MirroredVolume::pending()` returns **every** unacknowledged mutation, so a
   drained batch is always a *prefix* of the mutation sequence.
   `Repository::commit_manifest` writes each object before it swaps the head,
   so any batch containing the head swap contains everything written before it.
2. The driver applies one batch in one IndexedDB `readwrite` transaction, and
   never runs two flushes concurrently. A torn batch is not a state the durable
   store can be left in, and an older value of a key cannot land after a newer
   one.

The cost, stated plainly: a crash between a mutation and its flush loses the
tail of the sequence. The document reverts to an earlier *consistent* state,
never an inconsistent one. This is the same bargain ADR 0005 §5 already struck
by not calling `fsync` — with a smaller window, since a flush is scheduled
after every dispatched command and runs on the next microtask.

Pending mutations are coalesced per key (the head key is rewritten on every
save), which bounds memory by distinct keys rather than by writes. Coalescing
is safe only because a batch is atomic, so no intermediate value of a key is
ever observable in durable storage; `acknowledge` compares per-entry sequence
numbers, so a key rewritten while a flush was in flight is not acknowledged
away by that flush.

### 3. The browser's "local repository" is the volume

`save_local_repository` / `open_local_repository` / `scan_local_repository`
already exist, already carry a root path, and the browser UI already prompts
for one. Rather than adding a parallel set of browser-only commands,
`opendoc-app::repository::local_object_store` resolves what "local storage"
means per target: the filesystem on native, and the subtree
`repositories/<root>` of the browser volume on `wasm32`. The root the user
types is a name, not a path; `..` is refused so one repository cannot address
another's keys, and the recovery journal's subtree (`recovery/`) cannot be
reached from a repository root at all.

The command contract is therefore unchanged, `main.ts` is unchanged, and a
document saved in the browser is byte-identical to one saved natively — same
canonical CBOR, same manifest chain, same signatures.

### 4. The recovery journal stores frames as keys, not a segment as a value

`VolumeRecoveryJournalStore` implements the five byte-level methods of
`RecoveryJournalStore` over a volume, keyed
`recovery/<session id>/<frame index>` with the index zero-padded so byte order
is frame order.

A segment is deliberately *not* one value. Its first frame carries a whole
document snapshot and `append_segment` is called once per committed gesture, so
a single-value segment would rewrite that snapshot into IndexedDB on every
keystroke. One key per append makes a journal write a small insert — the same
shape the file store's append has.

### 5. Hydration happens once, before the first command

`storage_ready()` (a WebAssembly export returning a promise) opens the
database, reads every record, hydrates the volume, installs the recovery
journal and reports `{ persistent, entries, recoverySessions }`. The frontend
awaits it in `invoke.ts` immediately after the WASM module initialises; that
one `await` is the entire TypeScript side of this ADR. The IndexedDB binding
itself is Rust (`web-sys`), so no JavaScript owns storage semantics.

A runtime without IndexedDB — jsdom, a hardened profile, a private window that
refuses — is **not** an error. The volume keeps serving reads and writes from
memory, stops queueing mutations nothing will drain, and reports
`persistent: false`. That is exactly the behaviour the browser build had before
this ADR, so nothing regresses when storage is unavailable.

A command dispatched before hydration finishes (possible only if a caller skips
the `await`) is not overwritten by it: hydration skips keys the volume already
holds, so a live write beats the durable value it was about to replace.

### 6. One tab owns the store, and the others say they do not

Two tabs over one origin are two Rust runtimes over one IndexedDB database.
Each hydrates its own volume **once** (§5) and never sees the other's writes
again, so both go on flushing divergent views of the same keys: the last write
wins, neither tab notices, and the loser keeps a stale volume it will keep
flushing. `recent/documents` is one key for the whole origin, so the loss is
not theoretical — one tab's recents list replaces the other's outright.

A **Web Lock** named `opendoc-volume` decides it. `storage_ready` takes the
lock before hydrating; the tab that holds it owns the database, and a tab that
does not is memory-only and **reports that**, which is the same answer §5
already gives a runtime with no IndexedDB at all rather than a fourth
mechanism. The browser releases the lock when the tab goes away, a crash
included — which is why this is a lock and not a heartbeat key written into the
very store being contended for.

A waiting tab keeps mirroring rather than discarding, because its queue is
exactly what gets written if the owner closes and it is promoted; a second,
blocking lock request is what performs that promotion, hydrating under the same
rule as §5 (a durable value must not overwrite a live one, so the waiting tab's
own work outranks what the departed tab left). An unbounded queue nobody may
ever drain is itself a failure, so it is capped, and hitting the cap is
reported rather than absorbed.

A runtime with no Web Locks at all (jsdom, older browsers) carries on as
before — still right for a single tab — and says that a second tab would
overwrite it.

## Consequences

- Browser users get durable documents and crash recovery, through the same
  commands, the same repository format and the same UI as the native shell.
- The whole asynchrony problem is confined to one file,
  `crates/opendoc-wasm/src/storage.rs`. Everything above it —
  `MirroredVolume`, `MirroredObjectStore`, `VolumeRecoveryJournalStore`, the
  repository save/open paths — is target-independent and tested by
  `cargo test`, including `verify_object_store_contract` and a durable
  round trip that models a flush and a page reload. Only the binding itself
  needs a browser, and `npm run e2e` drives that in real Chrome.
- The memory cost of the volume is the whole repository, because it is a cache
  with no eviction. For a local-first document editor that is the right trade
  (a repository is megabytes), but it is a real ceiling, and it is why this
  adapter is not a general-purpose object store.

## Known limitations

- **A second tab is memory-only, for as long as the first one lives.** This is
  the cost of §6 and it is the honest one, but it is still a cost: two windows
  on two different documents is a reasonable thing to want, and only one of
  them is being saved. Lifting it means per-document ownership rather than
  per-origin, which is a much larger change.
- **A crash loses the unflushed tail** — at most the mutations of the command
  that was in flight. `storage_status()` exposes the watermark; `main.ts`
  surfaces the *durability* half of that report, not yet the watermark itself.
- **Nothing prompts for persistent storage.** A browser may evict IndexedDB for
  an origin under pressure. `navigator.storage.persist()` is the mechanism, and
  it needs a user-facing decision about when to ask, so it is deliberately not
  called here.
- **The audit projections still look at the filesystem.** The tombstone and
  candidate-head audits in `opendoc-app/src/audit_view.rs` construct a
  `LocalObjectStore` directly instead of going through `local_object_store`.
  In the browser they return nothing rather than erroring, so the audit view is
  incomplete there, not wrong. Routing them through the resolver is a
  one-line-per-site change in a file owned by another workstream today.
- **`compact_local_repository` does nothing in a browser.** Pack files are a
  filesystem optimisation; the volume reports `local_pack_files: false` and the
  command returns the store's "unsupported" error rather than pretending.
