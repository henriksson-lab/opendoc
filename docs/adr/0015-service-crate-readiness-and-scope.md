# ADR 0015: The Service Crate — Readiness Against the ADR 0004 Gate, and What It Owns

Status: accepted. Amended 2026-09-12 — see "Amendment: the runtime DTOs are
no longer local simulations" at the end. **Amends ADR 0004**: it lifts the "do not create
`opendoc-service` yet" decision and discharges the last unmet item of that
ADR's Service Creation Gate. Everything ADR 0004 says about *what the crate may
own* stands unchanged and is repeated here as the scope this crate is held to.

## Part 1 — the readiness assessment

ADR 0004 said: do not create `opendoc-service` until the local operation,
storage, merge and API boundaries are stable enough to expose over a network.
It listed seven conditions. Taken one at a time, against the repository as it
stands:

| # | Gate condition | Verdict | Evidence |
| - | -------------- | ------- | -------- |
| 1 | command parsing is typed and generated or owned from `opendoc-api` | **met** | `opendoc-api` owns the registry and the generator binary; `apps/desktop/commands.v0.json`, `apps/desktop/src/generated/*.ts` and the contract section of `docs/APP_API_CONTRACT_V0.md` are all written from Rust metadata, and `npm run verify` fails on drift. Nothing hand-writes a command schema. |
| 2 | app behaviour is split into services behind a small facade | **met** | `opendoc-app/src/lib.rs` is wiring and re-exports only; state is in `state.rs`, behaviour in ~20 typed `*_service.rs` / `*_commands.rs` modules. `OpenDocApp` is a facade, not the monolith ADR 0004 was written against. |
| 3 | operation application validates preconditions and postconditions | **met** | `opendoc-merge/src/validate.rs` refuses malformed payloads with a named warning per case; `opendoc-app/src/operation.rs` validates every envelope's source form, and `validate_operation_envelopes` runs over the whole history before a save. |
| 4 | merge tests cover small multi-actor streams | **met, and then some** | ADR 0007 replaced `(actor, seq)` ordering with a causal topological sort and character-level convergence through a sequence CRDT. `causal_convergence_tests.rs` carries a 2,000-seed byte-level convergence fuzz over four replicas, plus 2,000-seed insert and delete *oracles* that check the answer without the merge. |
| 5 | storage tests cover local and object-store-like backends through one trait | **met** | `verify_object_store_contract` is run against `LocalObjectStore`, `FlatObjectStore`, `MirroredObjectStore` and (behind the feature) `OpenDalObjectStore`. ADR 0008 added the second real backend and an explicit durability watermark. |
| 6 | runtime/session/permission DTOs are owned by `opendoc-api` | **met** | `opendoc-api/src/runtime.rs`. |
| 7 | relay semantics are specified as operation intake, candidate commit, or both | **not met before this document; met by it** | See below. |

Condition 7 deserves its own paragraph, because it is the one the existing code
does *not* satisfy and cannot. `relay_runtime_sync` specifies relay semantics as
**classification**: it sorts operation envelopes into accepted, deferred and
rejected by comparing a supplied `base_manifest` and checking the actor against
a supplied subject, and returns those three id lists. That is neither intake nor
commit — nothing is stored, nothing is ordered, nothing is answered. Condition 7
is a documentation obligation, and Part 2 of this ADR discharges it: relay
semantics are **durable operation intake with serialized commit**, specified
below and implemented in `crates/opendoc-service`.

**Verdict: the gate is met.** Two of the conditions were the real risk and both
have changed materially since ADR 0004 was written.

Condition 4 was the dangerous one. Before ADR 0007, concurrent character edits
converged to *wrong text* — every replica agreed on a corruption, so the
convergence suite was green while the merge was silently broken. A server built
on that would have faithfully replicated the corruption to every client and made
it durable, which is strictly worse than not having a server. That is no longer
true: offsets are resolved against the subsequence visible in the operation's
own causal context, so they are converted once into identities that do not
shift, and the insert and delete oracles check the result against an answer
computed without the merge.

Condition 5 was the other. A storage trait with one implementation is an
interface in name only. `MirroredObjectStore` over a `MirroredVolume` is a
genuinely different device — asynchronous underneath, with an explicit
`sequence()` / `durable_seq()` watermark — and it passes the same contract test.
ADR 0008's head-safety argument ("a durable head must never name non-durable
objects", and the two properties that make it true) is directly reusable as the
commit ordering a service needs, and is reused below.

### What the gate does *not* cover, and what that costs

Meeting the gate says the local boundaries are stable enough to expose. It does
not say the product loop is complete, and it is not. Two gaps remain, both in
`opendoc-app`, neither of which this ADR closes:

1. **`OpenDocApp` cannot ingest an operation it did not author.** Its entire
   public surface is `dispatch_command` plus lifecycle helpers; there is no
   `apply_remote_operations`. A local runtime can therefore talk to the service
   but cannot consume its fanout, so the browser and Tauri shells are not yet
   clients of this crate.
2. **`AppOperationEnvelope` is `pub(crate)`.** The service cannot write or read
   the app's operation-segment payload, so a repository the service writes is
   not openable by `open_local_repository` (see "The storage format" below).

Both are additive changes in a crate this work does not own. They are named
precisely in "What remains" so that whoever owns `opendoc-app` can make them.

## Part 2 — what the crate is

`crates/opendoc-service`. It owns exactly what ADR 0004 allotted it and nothing
else: authenticated HTTP/WebSocket transport, subject/session authentication,
server-enforced permissions, presence fanout, durable operation intake, and
commit serialization. Document semantics are not here. Operations are
`opendoc_merge::Operation`; ordering and convergence are
`opendoc_merge::merge_operations`; storage is `opendoc_store::Repository`.

### It does not depend on `opendoc-app`

ADR 0004 says the service must call the same typed operation and storage layers
as the local runtimes. It does — but through `opendoc-merge` and `opendoc-store`
directly, not through the app facade.

The reason is that the service dispatches no commands. A command is a *local
editing gesture*: it consults selection, undo checkpoints, coalescing windows
and projection caches, and it *produces* operations. The service consumes
operations that a client already produced. Routing that through `OpenDocApp`
would mean instantiating a facade whose entire purpose — turning gestures into
operations — is the part the service must not do, and dragging layout, render,
import, citations and spreadsheet evaluation into a network daemon's address
space to do it.

The dependency list is the argument: `opendoc-core`, `opendoc-format`,
`opendoc-merge`, `opendoc-store`. If document semantics ever start appearing in
this crate, that list is where it will show, and the `opendoc-app` dependency it
would need is the tripwire.

Consequence worth stating: nothing in the WASM or desktop build graph reaches
this crate, and this crate reaches nothing in theirs. `npm run build:wasm` is
unaffected by anything here.

### The transport is `axum` + `tokio` + `tungstenite`, deliberately

None of these were workspace dependencies. `tokio` was already in `Cargo.lock`
as an optional dependency of `opendoc-store` behind the `opendal` feature; the
rest are new.

The alternative was a hand-rolled HTTP/1.1 and RFC 6455 server over
`std::net::TcpListener` and a thread per connection, which for a small number of
clients would work and would add no dependencies. It was rejected. HTTP request
framing and WebSocket frame handling — masking, fragmentation, continuation
frames, interleaved control frames, the close handshake — are parsers sitting on
the untrusted side of the only network boundary this project has. Writing them
by hand inside the crate whose single purpose is that boundary is not a saving;
it is the one place in the repository where "use the battle-tested
implementation" is unambiguously right. `axum` brings `hyper` for the first and
`tungstenite` for the second, and `tungstenite` also gives the crate a real
client for its own protocol, which is what makes the two-client convergence test
possible without a browser.

The cost, stated: roughly a hundred transitive crates in `Cargo.lock` that
nothing else in the workspace uses. They are confined to a leaf crate that no
other crate depends on.

### Commit serialization is a thread, not a lock

Each open document is owned by one OS thread. `DocumentLog` — the merge base,
the operation log, the materialised document, the branch head — is reachable
from nowhere else. Connections are async tasks that send a request down an
unbounded channel and await a oneshot reply; the queue in front of that thread
*is* the serialization order.

A mutex would express the same property and depend on everyone taking it. A
single owner makes "two clients cannot interleave into an invalid head"
structural: there is no second path to the head to audit. It also fits the
storage layer as ADR 0008 left it — `ObjectStore` is synchronous by design, so a
commit is a blocking call that has no business on an async executor's worker
thread.

### The storage format, and head safety

One document is one branch in an `opendoc-store` `Repository`: a chain of
manifests, each naming the operation segment that produced it and a snapshot of
the document at that point. The genesis manifest names no segment and its
snapshot is the merge base.

This is not a new format. It is the manifest/segment/snapshot chain
`opendoc-app` already writes, with `opendoc_merge::Operation` inside a segment
entry where the app puts `AppOperationEnvelope`. The difference is forced, not
chosen: `AppOperationEnvelope` is `pub(crate)`. Until it is public, a repository
the service writes uses `source_format: "opendoc.service-document.v0"` and
carries `opendoc_core::Document` rather than `AppDocument`, and
`open_local_repository` cannot read it.

Write ordering is ADR 0008's rule, unchanged: snapshot object, then segment
object, then manifest object, then the compare-and-swap that moves the head. A
crash at any point leaves objects nothing points at — garbage, never a dangling
head. `every_object_the_durable_head_names_is_already_durable` walks the whole
chain and asserts it rather than trusting the comment.

A lost compare-and-swap **refuses**. The service does not retry and does not
fall back to a candidate head: another writer moved the head, and the service
cannot know whether that writer's operations belong in its merge base. Silently
retrying would be exactly the last-writer-wins ADR 0004 forbids.

### Why the server re-merges from genesis

The server keeps the genesis base and the entire operation log, and recomputes
`merge_operations(base, log)` on every commit.

This is not laziness, it is ADR 0007's stated limit taken seriously. That ADR
buys structural convergence by reconstructing per-character identities from
(base, operation set) at merge time and *not* persisting them — and pays for it
with: "once a merge result is written back as plain text and becomes the new
base, a later-arriving operation that predates it cannot be placed by identity
any more; it degrades to positional (clamped) application." Materialising each
commit against the previous commit's output would make every concurrent
operation a late arrival against a collapsed base. The server would reintroduce,
at the one point where it is hardest to notice, exactly the corruption ADR 0007
exists to remove.

The cost is real: a commit is O(log length), so a document's lifetime is
O(commits²) of merge work. It is bounded work on bounded data and it is correct,
which is the right order to get those in. See "Known limitations".

## What the service actually enforces

Everything in this list is decided from server state. None of it is decided from
anything the client sent.

- **Identity.** A subject exchanges a long-lived API key for a short-lived
  session token. The directory stores only the SHA-256 of the key; comparison is
  constant-time; an unknown subject and a wrong key return the identical error.
- **Authorship.** Every subject is bound to one `ActorId` in the directory, and
  every submitted operation's `id.actor` must equal it. Since `OperationId` is
  what the causal order and last-writer-wins are computed from, this is the
  single most load-bearing check here: an actor id a client could choose is an
  actor id a client could impersonate. A subject cannot be registered onto an
  actor id another subject already holds.
- **Permission, re-read per submit, from durable storage.** Roles are
  Viewer < Commenter < Editor < Owner, stored under the service's own
  `service/permissions/` namespace — deployment state, never document source,
  never a manifest. The role is read again on every submit, not captured when
  the socket opened, so a revoked editor's next keystroke is refused rather than
  the one after its next reconnect. Revoking read also closes connections that
  are already open, and drops the subject's sessions.
- **Sequence density, on the way in and on the way out.**
  `VectorClock::observed` reads `seq >= n` as "every one of that actor's
  operations up to n". That is only true if per-actor sequences are dense, so an
  operation must be exactly its actor's last plus one; gaps are refused, not
  stored. `DocumentLog::load` makes the matching check against storage and
  refuses a stored log that has a gap. Together they let the causal-honesty
  check below be an array index rather than a scan: the server keeps, per actor,
  the running maximum Lamport timestamp over its first *n* operations, so both
  "is this operation in the log" and "what is the highest timestamp this clock
  could have observed" are O(1).
- **Causal honesty.** An operation may not claim to have observed an operation
  the log does not contain, and may not claim a Lamport timestamp above one past
  the highest it observed. Without the second check a client sets
  `lamport: u64::MAX` once and wins every last-writer-wins contest on that
  document for ever — a real attack on a collaborative server, and cheap to
  block.
- **History immutability.** An operation id already in the log may be resubmitted
  only with a byte-identical payload — that is a retry, and it is acknowledged
  without committing anything. A different payload under a logged id is refused.
- **Durability before acknowledgement.** A client is told "accepted" only after
  the branch head names a manifest that names a segment containing its
  operations. There is no "accepted, will be written" state to reason about.
- **Existence is not leakable.** A caller with no grant gets the same error for a
  document that exists and one that does not, so the API is not an oracle for
  document ids.
- **Presence is server-attested.** Subject, actor and role in a `PeerView` are
  server state. A client contributes only its own display name and cursor
  anchor. Presence never touches storage (ADR 0004: presence stays ephemeral and
  outside signed document state).

## What it still trusts the client for

Stated plainly, because a list of enforcement is only honest next to this one.

- **Operation payloads are not semantically authorized.** The server checks that
  you are an editor; it does not check that a `DeleteBlock` targets a block you
  are allowed to delete, or that a `Commenter` sent only comment operations. The
  `Action::Comment` rank exists and is enforced for nothing yet: distinguishing
  comment-shaped operations from document-shaped ones needs a classification of
  `OperationKind` that belongs next to the operation vocabulary in
  `opendoc-merge`, not here.
- **Suggestion and comment authorship inside payloads.** A `Comment`'s author
  field is whatever the client put in it. Only the *operation's* actor is
  attested.
- **`context: None` is accepted.** ADR 0007 makes it the conservative reading
  ("concurrent with everyone"), and refusing it would break replay of anything
  written before that ADR — but a client that omits context sidesteps the
  causal-honesty checks by having nothing to check.
- **Cursor anchors are unvalidated strings.** They are relayed, not resolved
  against the document.
- **The token in the WebSocket URL.** A browser cannot set an `Authorization`
  header on a WebSocket handshake, so the session token rides in the query
  string, where access logs and `Referer` can see it. That is why sessions are
  short-lived and individually revocable rather than long-lived keys, but it is
  a real exposure, not a solved problem.

## What this does not do

- **No TLS.** The server speaks `http://` and `ws://`. It is meant to sit behind
  a terminating proxy, and until it does, every credential on this wire is in
  the clear.
- **No user management.** Subjects are provisioned by an operator, from the
  environment. There is no registration, no password reset, no rotation, no
  lockout, no federation. `IdentityService` is a directory, not an identity
  provider, and the API key is a shared secret, not a password — there is
  deliberately no password hash here, because a password store needs Argon2 and a
  policy, and this needs neither yet.
- **No rate limiting, no connection quota, no request size limit** beyond a cap
  of 512 operations per submit.
- **No audit events.** ADR 0004 lists "service audit events" in the crate's
  scope. They are not implemented.
- **No document lookup by UUID/DOI over service-visible indexes.** Also in
  ADR 0004's scope, also not implemented; documents are addressed by uuid only.
- **No candidate-head reconciliation.** The service refuses a lost
  compare-and-swap rather than writing a candidate head. With one writer per
  document there is no legitimate way to lose that race, so a loss means an
  external writer and refusing is the safe answer — but ADR 0004's
  "deterministic candidate-head recovery" is therefore not exercised here.
- **No spreadsheet or blob operations.** The log carries
  `opendoc_merge::Operation` only. `opendoc-app`'s spreadsheet and blob
  envelopes have no service path.
- **Not multi-process.** One service process per object store. Two processes
  over one store would each own a document thread and fight over the head; the
  compare-and-swap would make that loud rather than silent, but it is not a
  supported deployment.

## Validation

ADR 0004 listed five validation requirements. Four apply to this crate, and each
has a test that fails without it (see the mutation evidence in the workstream
report):

- *"A service-mode command cannot mutate a document without passing through the
  same typed operation validation used locally."* The service applies operations
  only through `merge_operations`, and an operation set that does not merge is
  refused before anything is written.
- *"Permission-denied service requests leave source state and branch heads
  unchanged."* `a_viewer_receives_commits_but_cannot_write` asserts the commit
  sequence is unmoved after the refusal.
- *"Accepted service operations survive restart and reopen from storage."*
  `accepted_operations_survive_a_full_service_restart` shuts the server down,
  builds a new service, new session table and new permission cache over the same
  object store, and requires byte-identical canonical CBOR.
- *"Conflicting branch-head updates produce deterministic candidate-head recovery
  instead of silent last-writer-wins."* Partially: `a_head_moved_by_another_writer_makes_the_commit_refuse`
  proves it is not last-writer-wins. Candidate-head recovery is not implemented —
  see above.

The test that matters most is not on that list.
`randomised_concurrent_sessions_converge_byte_identically_over_the_transport`
runs 24 seeds of three clients editing one run concurrently over real TCP
sockets, and after every seed requires all three replicas, a fourth client
bootstrapped from the welcome alone, and the server's own materialised document
to encode to identical canonical CBOR — with a guard that at least 90% of seeds
actually changed the document, so it cannot pass vacuously.
`opendoc-merge` already proves convergence in process over 2,000 seeds; this is
not a second copy of that proof. What it adds is that nothing *between* the
replicas — the socket, the JSON encoding, the commit serializer, the fanout, the
welcome — loses or reorders an operation in a way the in-process proof would not
see.

## Known limitations

- **O(commits²) merge work over a document's lifetime**, for the reason in "Why
  the server re-merges from genesis". The fix is a compaction that rewrites the
  base and states a cutoff before which late operations are refused rather than
  silently re-anchored — which is a decision about the product, not an
  optimisation, and belongs in its own ADR.
- **The whole log is in memory per open document**, and the welcome sends all of
  it. Fine for a research prototype's documents; not fine for a long-lived one.
  An incremental welcome needs the client to say what it already has, which the
  protocol has no message for yet.
- **A document thread is stopped only when nothing is using it.** It is
  reclaimed when no clone of its handle exists anywhere in the process *and*
  nothing has asked for the document within
  `OPENDOC_SERVICE_DOCUMENT_IDLE_TIMEOUT_MS` (default 15 minutes). The first
  half is not a heuristic and cannot be relaxed: stopping a thread while a
  handle lived would let the next open start a second thread on one branch
  head. So a document a client is connected to is never reclaimed, however
  long it sits idle — a process whose documents are all in use holds every one
  of those threads, and `max_open_documents` is what bounds that.
- **A connection's outbound queue is unbounded.** A client that connects and
  then stops reading accumulates every commit and presence update in memory
  until it disconnects. Bounding it means deciding what to do when it fills —
  drop the slow reader, or block the commit — and that is a policy decision,
  not a constant.
- **`OpenDocService::document` holds the registry lock across the storage read**
  that loads a document on first use, and across the reclamation pass that may
  precede it. Correct — the wait for a stopping thread has a deadline, and a
  document thread never takes the registry lock, so neither can hang it — but
  it serialises first opens across documents.
- **Grant changes are cached in-process.** A second service process over the same
  store would not see them.

## Consequences

- ADR 0004's prohibition is lifted. Its scope list is not: the crate owns
  network and service concerns only, and the dependency list is the check.
- `opendoc-api`'s runtime DTOs are still local simulations and must still be
  documented as such. They are not what this service speaks, and making them
  into requests-and-answers against a real service is a contract change that is
  named in the workstream report and deliberately not made here.
- Permissions in Tauri and WASM remain advisory, exactly as ADR 0004 said, until
  those runtimes become clients of this service — which needs the two
  `opendoc-app` changes named in Part 1.

## Amendment: the runtime DTOs are no longer local simulations

Dated 2026-09-12. The "Consequences" bullet above said `opendoc-api`'s runtime
DTOs are still local simulations and that making them into requests and answers
against a real service is a contract change deliberately not made here. That
change has now been made, and this section records its shape. Nothing else in
this ADR changes.

Four things were wrong with those DTOs, and each is now fixed:

1. **`authorize_runtime_command` took the caller's own grants.** It no longer
   takes any. An `OpenDocAuthorizationDecision` now names `decided_by`:
   `runtime-capability` when a local runtime decided it from its own
   capabilities (permissions there are advisory, ADR 0004, so no subject and no
   grant are involved at all); `service-answer` when it came from the role the
   service attested; `service-answer-missing` when the runtime defers to a
   service and has no answer from one, which refuses. There is deliberately no
   fourth state meaning "the client worked it out". The service's answers reach
   the app as an `OpenDocServiceSession` — subject, actor, document uuid, role,
   peers and the acknowledged sequence watermark — written only by
   `OpenDocApp::join_collaboration_session` and the presence and
   acknowledgement helpers beside it, all of which a transport calls with what
   the socket delivered. `dispatch_command` reads it from the app, never from
   an argument, so there is no argument left that could widen it.

2. **`OpenDocSyncRelayResult::deferred_operations` had no meaning.** It is gone.
   The result now carries an `outcome` of `accepted`, `refused` or `retried` —
   the only three answers this service gives a batch — plus the operation ids in
   each class. The command is a **preflight**, documented as such: it applies
   the checks this crate applies (the session's actor binding, per-actor
   sequence density, history immutability) to a batch a transport is holding, so
   the transport learns about a batch it must not send instead of discovering it
   as a rejection. `the_sync_preflight_and_the_service_give_the_same_answer`
   drives both halves and requires them to agree.

3. **`OpenDocPresencePeer` lacked `actor` and `connections`.** Both are now
   there, matching `PeerView`. Without `actor` a client cannot map a cursor to
   the operations that produced it; without `connections` one person in two tabs
   looks like two people.

4. **`MultiUserService` meant "permissions evaluated locally".** The profile now
   carries `permission_authority`, which is `local-advisory` or `service`;
   `permissions_enabled` is gone, because it read as "this runtime evaluates
   permissions", which is the thing no runtime may do.

`OpenDocPermissionGrant` was deleted rather than adapted. The service's grant
table is keyed by role, not by an action list, so a share invite now asks for a
`requested_role` and is explicitly a *request*: only the service writes grants.

`OpenDocServiceRole` restates this crate's `Role` because `opendoc-api` is in
the WASM dependency graph and this crate deliberately is not. A restated
definition is only as good as the test that compares it, so
`the_app_and_the_service_agree_on_roles_and_what_each_allows` checks every
variant's wire string, every action's minimum role, and the ordering.

### And the sequence counter that was serving two identities

`opendoc-app` numbered operation envelopes and document operations from one
counter, so an envelope carrying no typed operation — an undo marker, a blob
upload, a spreadsheet edit — consumed an `OperationId` and left a hole in its
actor's operation sequence. This service refuses a non-dense sequence rather
than storing one (see "Sequence density" above), so a user who attached a file
mid-session and kept typing was refused for it.

There are now two counters. `next_envelope_seq` numbers journal entries;
`next_operation_seq` numbers `OperationId` and nothing else.
`AppOperationRecord::seq` is the envelope's and is no longer required to equal
the operation's — that requirement was what forced the collision — while the
record's *actor* must still match its payload's.
`the_service_accepts_a_stream_interrupted_by_non_document_work` drives a real
`OpenDocApp` through typing, a blob, a spreadsheet edit, an undo and more typing
and requires this service to accept the resulting batch.

A repository written before the split keeps its gaps: renumbering would rewrite
the causal contexts that name those ids, and both live inside content-addressed
objects a manifest chain already commits to. It is read exactly as written, and
`legacy-operation-sequence-gap` names it on the way in rather than letting it be
read as though it were the dense stream a service requires. The recovery segment
(ADR 0005) now records both watermarks in its header, and its format string went
to `v1`; a `v0` segment is still read, because in `v0` one counter numbered both
and its shared numbering *is* both watermarks exactly, and the replay says which
format it found.

## Amendment: the limits this service owns, and the two claims that were false

Dated 2026-09-13. Nothing above changes except where this section says so.

### The submit cap is a deployment setting, and it travels

"No rate limiting, no connection quota, no request size limit **beyond a cap of
512 operations per submit**" described a constant one crate held and nothing
told anybody about. Neither client chunked to it, so a client with more than
512 unsent operations — a replayed disconnection, a large paste, an import —
built one batch the service refused, and the refusal wedged the session for
good (PLAN88 P1-8).

The cap is now a property of the deployment
(`with_max_operations_per_submit`, `OPENDOC_SERVICE_MAX_OPERATIONS_PER_SUBMIT`)
and **every welcome carries it**. A client chunks its outbox to the number that
arrived. This is the only arrangement in which there is one definition: a
client that restated 512 would be a second definition of somebody else's limit,
and one that guessed would either submit needlessly small batches or be refused
for ever. `ServerMessage::Welcome` gained `max_operations_per_submit`, and the
wire fixture carries it, so a client that cannot read it fails the tripwire.

### Two new limits, both closing the same chain

`POST /v1/documents` was authenticated and authorized by nothing, and each
document it created took an OS thread that nothing then stopped — with the spawn
ending in `.expect("spawning a document thread")` **inside the document
registry's mutex**. A process out of threads or file descriptors therefore did
not fail to open one document: it panicked under the lock and poisoned the
registry, after which every document open on that process answered 500 until it
was restarted.

Three changes, and all three are needed:

- **The spawn is fallible.** `spawn_with_log` returns a `ServiceResult`. An
  operating system that will not give this process a thread is a condition to
  report.
- **`max_open_documents`** (default 1024) bounds how many threads one process
  will take. Reaching it is a refusal a caller can read, and one that lifts
  again: see "Reclamation, and why it is not a timer" below.
- **`max_documents_per_subject`** (default 256) is the authorization that
  endpoint was missing, charged durably before the genesis commit so a restart
  does not hand everybody a fresh allowance. The record is keyed by the SHA-256
  of the subject, so a subject name can never decide where this service writes.

### Reclamation, and why it is not a timer

The three changes above bounded the threads without ever giving one back, and
that was the whole of the remaining problem: a process that reached
`max_open_documents` refused every new document for the rest of its life, even
with a thousand documents nobody had touched in a week. Eviction was left out
for a reason that was correct as far as it went — the registry hands out clones
of a handle, so dropping an entry while a clone is alive would let a second
thread be spawned for the same document, and two writers on one branch head is
the one thing this design exists to make impossible.

The way out is not to evict more carefully; it is to make the condition
answerable. `DocumentHandle` is now one `Arc` shared by every clone, so
`is_solely_held` asks the registry's own clone whether it is the last one in
the process — and the registry asks it *while holding its own lock*, which is
what makes the answer usable rather than stale the instant it is read: the only
way to obtain a clone is `OpenDocService::document`, which needs that same
lock, so while it is held the count can fall but never rise. A document is
reclaimed when that answer is yes and nothing has asked for it within the idle
window; the handle is dropped, the thread's inbox closes, and the entry is only
forgotten once the thread has been *seen* to leave its loop.

If the thread does not stop within five seconds — which, the condition above
being true, would mean this service was wrong about something — the uuid is not
freed. It is held in a retiring state that refuses to open the document at all,
with a message saying its thread has not exited and that no second thread will
be started for one branch head. That is the shape of the whole design: where it
cannot be certain, it refuses rather than serving a document twice.

What it costs: reopening a reclaimed document reloads its log from the object
store, so the idle window is a bet that a client which just disconnected is
unlikely to come straight back. Fifteen minutes by default, configurable, and
zero is allowed — it means "keep nothing warm", not "stop threads that are in
use", which remains impossible to ask for.

### `Action::Comment` is enforced, so `Commenter` means something

"The `Action::Comment` rank exists and is enforced for nothing yet" was true:
`submit` required `Action::Write` whatever the batch contained, so a Commenter
could not submit a comment and was a Viewer with a different word on the pill.
The permission a batch needs is now decided from the batch: `Action::Comment`
when every operation in it is an annotation — comment threads, replies, comment
lifecycle, and `AddSuggestion`/`UpdateSuggestionInsertContent`, none of which
reach the block tree — and `Action::Write` otherwise, including for a batch
that mixes the two and for `AcceptSuggestion`, which is how a suggestion's
content *enters* the document.

The classification is an exhaustive `match` in `document.rs`. This ADR said it
belongs next to the operation vocabulary in `opendoc-merge`, and that is still
where it belongs; it is here because the exhaustive match makes a new
`OperationKind` a build failure rather than a silent widening, and moving it is
an `opendoc-merge` change.

### The author inside a comment payload is bound to the subject

This ADR's "What it still trusts the client for" says: "A `Comment`'s author
field is whatever the client put in it. Only the *operation's* actor is
attested." That was true, and it was the worse half of the pair — the actor
binding covers *who submitted*, and `Comment::author` is a different string:
the one a reader sees, merged into the document and covered by the signature.
A subject could sign in honestly as itself and attribute a comment to somebody
else, permanently.

It is now checked. Every author field a submitted payload carries —
`AddCommentThread`'s comments, `AddCommentReply`'s comment, `AddSuggestion`'s
suggestion — must equal the session's authenticated subject, or the batch is
refused and nothing is written. The check sits beside the actor binding in
`validate_batch`, and the classification that finds those fields is the same
exhaustive `match` that decides `Action::Comment` above, so a new authored
payload is a build failure rather than an unchecked field.

**Bound, not rewritten.** Rewriting the field would leave the operation the
server logs different from the one the client holds under the same id, and
neither side would ever be told — divergence in the one place nobody looks.
Refusing leaves exactly one payload per id, and the refusal names both the
claimed author and the subject it should have been.

**What this costs, and how it is paid.** The author a client must send is now
the *subject*, not a display name — and nothing supplied one. `dispatch_command`
takes the author from the command argument, and the shell filled that from
`state.authorName`, which is `runtime.subject ?? "Local user"` and is therefore
`"Local user"` in a browser whose host injected no runtime config. So this
check, shipped alone, would have refused every comment and every suggestion
made inside a session — and with the refusal recovery in ADR 0018's amendment
each one would have cost three resynchronisation attempts and then the session.

`collab.ts` closes that: it adopts the subject the welcome attested as the
document's author name for the life of the session and restores the previous
one on disconnect. This is the page copying a server answer rather than
deciding one, which is what that module is for. It is deliberately *not* an
`opendoc-app` change — the app keeps taking the author from its argument, and
the transport supplies the argument's value, so nothing in the core learns
about sessions it does not already know about.

`a_comment_a_real_app_authored_reaches_the_service` is what makes that a
statement rather than a hope: it drives a real `OpenDocApp` over a real socket
and requires both halves — its comment under the attested subject is accepted,
and the same app under any other name is refused. No test asked that question
before, which is exactly why a check and a shell could disagree about the one
string they both touch.

**Two related gaps stay open:** `UpdateCommentBody` and `DeleteComment` are
classified as annotations and carry no author, so a Commenter may still edit or
delete *another* subject's comment (the service would have to read the existing
comment's author out of the materialised document to refuse that); and a
repository written before this check keeps whatever authors it was given, which
is read back unexamined.

### "Drops the subject's sessions" is now true

"Revoking read also closes connections that are already open, and drops the
subject's sessions." The first clause was implemented; the second named
`IdentityService::close_sessions_for_subject`, which had **no caller at all**,
so a revoked subject's bearer tokens went on working until they expired.
`OpenDocService::set_grant` now calls it when a grant is revoked. Sessions are
per subject rather than per document, so this is wider than the grant that
changed — the subject must sign in again everywhere — which is the reading this
ADR wrote down and the safe direction to be wrong in.

### Cursor anchors are bounded, and an unchanged presence fans nothing out

"Cursor anchors are unvalidated strings" stands, with one limit added: an anchor
is cut to `MAX_CURSOR_ANCHOR_CHARS` (256) before it is stored. It is cloned into
a `Presence` frame per connection on every presence change, into outbound
queues this service still does not bound, so an unbounded anchor was
memory-exhaustion amplification a read-only Viewer could trigger. A presence
update that changes neither the display name nor the anchor is now recorded
without a broadcast, for the same reason.

### Grant writes are serialized

`PermissionService::set_role` was a read-modify-write with no
compare-and-swap: two concurrent share requests could both read the same table
and both write their own version of it, losing one grant — and deciding "this
would leave the document with no owner" from a table that was already stale. A
single write lock is now held across the whole of `set_role` and `seed_owner`,
including the authorization. Multi-process deployment is still unsupported, so
an in-process lock is the whole of the answer.

### The session table is swept

`authenticate` drops an expired session as it refuses it, which only ever
reaches tokens somebody still presents. `open_session` now sweeps the table
first: signing in is the one moment that is both cheap to do it in and
guaranteed to happen while the table is growing.
