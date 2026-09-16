# ADR 0018: One Protocol, Two Transports — Where the Collaboration Socket Lives

Status: accepted. Implements parity CO-15 (transport) and CO-18 (presence), and
closes the gap ADR 0015 named as "connecting the desktop/browser shells is
still not done: nothing in `main.ts` or `src-tauri` speaks the protocol".

Does not amend ADR 0004 or ADR 0015; it is the first thing built *on* them.

## Context

Everything below the shell existed. `opendoc-service` enforces identity,
authorship, per-submit permissions, causal honesty and durability before
acknowledgement, and its own tests drive three real `OpenDocApp` clients plus a
late joiner to byte-identical convergence over real TCP. `OpenDocApp` can
ingest fanout: `join_collaboration_session`, `apply_remote_operations`,
`local_operations_after`, `apply_service_presence`, `apply_service_role`,
`acknowledge_service_operations`, `leave_collaboration_session`.

Nothing opened a socket. No shell spoke the protocol, and `OpenDocPresencePeer`
reached the app but nothing rendered it.

Three facts constrain how that gets fixed.

1. **`opendoc-service` must not enter the WebAssembly dependency graph.** Its
   client is built on `tokio` and `tungstenite`; behind them sit roughly a
   hundred crates nothing else in the workspace uses. ADR 0015 states the
   confinement ("nothing in the WASM or desktop build graph reaches this
   crate") and ADR 0004's boundary is the reason: a network daemon's
   dependencies have no business inside the browser core. The tripwire is
   `cargo tree -p opendoc-wasm --target wasm32-unknown-unknown`, and this work
   keeps that output free of `opendoc-service`, `tokio`, `tungstenite`, `axum`
   and `hyper`.
2. **The collaboration surface on `OpenDocApp` is Rust methods, not commands.**
   `apply_remote_operations` and its neighbours are not in
   `opendoc-api`'s registry and cannot be reached through `dispatch`. So "the
   browser's socket feeds `apply_remote_operations` through the existing
   command surface" — the obvious answer — is not available: there is no
   command to feed. Adding one is an `opendoc-api` change this workstream does
   not own (see "What this needs from other crates").
3. **TypeScript owns the DOM; Rust owns semantics.** A TypeScript socket that
   moves opaque frames does not violate that. One that reads a frame does.

## Decision

**One protocol, two transports, one UI.** The service's wire protocol is the
only definition of what a client may say. Where the socket *lives* differs per
runtime, because what the runtime can link differs per runtime. What the page
renders does not differ at all.

### Browser: TypeScript owns the socket, Rust owns every byte inside it

`apps/desktop/src/collab.ts` opens the `WebSocket`, retries it, reads the caret
out of the DOM, and draws the region. It hands each frame to
`crates/opendoc-wasm/src/collab.rs` as a string and gets a status DTO back. It
never parses a frame and never composes one: the frames it sends are the
strings `collab_outbox()` returned.

The WebAssembly boundary is six functions — `collab_begin`, `collab_frame`,
`collab_closed`, `collab_outbox`, `collab_cursor`, `collab_leave` (plus
`collab_status`) — and `CollabDriver` behind them is a state machine over
frames with no socket, no timer and no browser in it, which is why all of it is
tested by `cargo test -p opendoc-wasm` on the host.

**Why not `web_sys::WebSocket`, with Rust owning the socket as it owns
IndexedDB?** ADR 0008 put the IndexedDB binding in Rust precisely so no
JavaScript would own storage semantics, and the same argument seems to apply
here. It does not, for a specific reason: frames would then arrive in
`wasm-bindgen` closures that can fire at any time, including while `dispatch`
holds the `RefCell` in `with_app`. The re-entrancy guard there returns an error
— so a frame arriving mid-command would be **dropped**, silently, and this
replica would stop agreeing with the server. JavaScript cannot interrupt a
synchronous call into WebAssembly, so a frame handed over from a `message`
listener always arrives when no command is in flight. The TypeScript socket is
not a concession; it removes a correctness hazard, and it costs nothing in
ownership because a socket that moves opaque strings decides nothing.

**Why a 250 ms outbound pump rather than a hook on each edit.** A local gesture
becomes an operation inside `dispatch`, and the module that routes gestures
(`actions.ts`) belongs to another surface. Polling costs one
`local_operations_after` per tick and cannot miss an edit; a hook would have to
be installed on every path that can author one.

### Native: Rust owns the socket, using the service crate's own client

`apps/desktop/src-tauri/src/collab.rs` uses
`opendoc_service::client::{ServiceClient, DocumentSession}` — the client that
ships beside the server and is exercised by its convergence suite. One OS
thread per session runs a current-thread runtime, owns the `DocumentSession`,
and reaches the document through the same `Mutex<OpenDocApp>` the `dispatch`
command uses, holding that lock only across synchronous calls and never across
an `await`.

Two things make this the better half of the trade where it is available:

- **The session token never enters the webview.** The whole credential exchange
  is native, so a page bug cannot leak it. In the browser the token is
  necessarily in the page (and in the socket URL — ADR 0015 already records
  that exposure).
- **No second client.** The protocol has one reader and one writer on this path,
  both maintained next to the server.

The page's surface is identical: `collab_connect`, `collab_disconnect`,
`collab_cursor`, `collab_status` as shell commands, and the same status DTO
pushed back as `opendoc://collab-status` events. `collab.ts` picks by
`isTauri()`, exactly as `invoke.ts` picks between `dispatch` implementations.

### What is copied, and what checks the copy

The wire *envelope* is restated in `opendoc-wasm` (`ServerFrame`,
`ClientFrame`, `WirePeer`, `WireOperationId`, `PROTOCOL_VERSION`). This is the
same trade the repository already makes twice: `opendoc-api` restates
`OpenDocServiceRole` and `opendoc-app` restates `SERVICE_DOCUMENT_FORMAT`, each
with a test in `opendoc-service` that compares them.

What is **not** copied: an `Operation` is `opendoc_merge`'s own type, because
`opendoc-merge` is in the WebAssembly graph. Only the envelope is duplicated,
and the payload — the part convergence depends on — has exactly one definition.

The copy is checked in bytes, from both sides:

- `crates/opendoc-service/wire/protocol-frames.json` holds every frame of the
  protocol with every field populated. It is **generated**, never hand-edited:
  `OPENDOC_WRITE_WIRE_FIXTURE=1 cargo test --release -p opendoc-service
  wire_fixture` rewrites it, and `the_checked_in_wire_fixture_is_what_these_types_serialize_to`
  fails when it is stale — the same shape as the generated command contract.
- `opendoc-wasm`'s `the_restated_frames_read_what_the_service_crate_writes`
  parses that file with the restated types and checks the values field by
  field, in both directions (a `Submit` it composes must equal the one the
  fixture holds).

A protocol change that either side does not follow fails one of those two.

The status DTO the page renders is also stated twice — once in `opendoc-wasm`,
once in `src-tauri` — because the two runtimes produce it in different
processes. Both pin the field list in a test
(`the_status_serializes_with_the_fields_the_frontend_reads`,
`the_status_carries_the_fields_the_frontend_reads`) so `collab.ts` can keep one
type.

### The service now has an opinion about browsers

A page could not reach this service at all before, and this is not a detail of
the UI: it is the reason the whole browser path could not exist.

- **CORS.** The token exchange, document creation and description are
  cross-origin `fetch` calls, so without `Access-Control-Allow-Origin` the
  browser refuses to hand the page the response.
- **Cross-site WebSocket hijacking.** The same-origin policy does not cover a
  WebSocket handshake: any page anywhere can open `ws://` to this service, and
  the `Origin` header is the only thing that says which page did.

`crates/opendoc-service/src/origin.rs` is one exact-match allowlist used for
both answers, configured by `OPENDOC_SERVICE_ORIGINS`. **The default is deny**:
an unconfigured service is reachable by programs and by no page at all. That is
the safe direction to be wrong in — a missing origin is a page that cannot
connect, which is loud, where a permissive default is a page somewhere else
that can, which is silent. Matching is exact (no wildcards, no suffixes, and
`null` and `*` are never stored), and a request carrying no `Origin` is
untouched, which is every existing non-browser client.

### A dropped socket

The same four steps in both runtimes, in this order, because the order *is* the
answer:

1. Take what this replica holds **before** joining: joining replaces the
   document with the service's.
2. Join from the welcome — merge base plus the whole operation log — so this
   replica *is* the service's document. ADR 0007 makes the merged bytes a
   function of exactly those two things.
3. Read the acknowledged watermark back out of the log the welcome carried. The
   service enforces dense per-actor sequences, so whatever of this actor's work
   is in that log is a prefix of it, and its highest sequence number is what the
   service has made durable. There is no need to remember it across the drop.
4. Replay the tail the service never got — as *operations*, with their original
   ids and causal contexts, not as re-authored gestures. The ordinary outbox
   then submits them.

Step 4 is the only one that can fail, and it fails loudly: work that cannot be
replayed onto the service's log is work this replica cannot put on the wire, and
the notice says so and marks the session unresumable. Dropping it silently would
lose a user's typing; keeping it silently would leave this replica permanently
disagreeing with everyone else.

Retries are bounded (four backoff steps, then eight attempts in total), and
this is where the honesty costs something. **A browser is never told why a
WebSocket handshake failed** — the HTTP status of a refused upgrade is
deliberately hidden from the page — so an expired session, a revoked grant and
an unplugged cable are one event in the browser. Retrying for ever would show
"Reconnecting…" against a service that will never answer. After the cap the
pill says the session is over, how much work never left, and that a reconnect
is the way to find out which of the three it was. The native path can read the
status (`ServiceError::Unauthenticated`/`Forbidden`/`NotFound` are not
resumable) and ends the session immediately instead of retrying.

A close the service *sends* is different from a socket that merely dies:
`forbidden` and `unauthenticated` will refuse the next handshake identically, so
those go straight to "Disconnected" with the service's own words. Saying
"reconnecting" for them would be a lie.

### Presence

`OpenDocPresencePeer`, as the service attested it, read back out of the app
through `OpenDocApp::service_session()`. The region draws one chip per peer:
initials, display name, the role the service granted, the connection count
(one person in two tabs is one peer with two connections), and the peer's
cursor anchor. Chip colour is derived from the **actor id** rather than the
display name, because the actor id is server state and a name a peer chose
could collide or change mid-session.

This client's own caret is contributed as an opaque `block:inline:offset`
string. The service relays a cursor anchor and never resolves it (ADR 0015), so
its shape is the client's business, and mapping a caret to and from the DOM is
squarely TypeScript's half of the boundary.

**In-document remote carets are deliberately not drawn yet.** Placing a caret
marker inside the `contenteditable` host would corrupt `editor.ts`'s DOM-to-
offset mapping — the thing that makes typing land where the user aimed — so it
has to be an overlay layer positioned outside the editable subtree. That is a
separate piece of DOM work, and the participant list is what CO-18 asks for at
minimum.

### Listener discipline

The rule `bindings.ts` exists to protect, applied here: exactly one delegated
click listener, bound once on `app` (which the page creates and never
replaces), dispatching on `data-collab-action`. The region is re-rendered by
overwriting `innerHTML`, which therefore cannot stack a handler. A socket's
handlers are *assignments* on a freshly created socket and are set to `null`
before it is closed, so a late event cannot fire into a torn-down session. The
pump and retry timers are cleared before they are set. The native status
listener is bound once for the process's lifetime — unsubscribing on disconnect
and resubscribing on connect is precisely the re-runnable path that stacks
listeners.

## Two bugs this found

Stated because both were invisible until something outside a test tried to use
the service.

- **The service binary served nothing.** `main` called
  `RunningServer::shutdown()`, which *sends* the shutdown signal and then waits
  for the task, so the process printed "listening on …" and immediately tore
  the listener down. Nothing noticed because the only client this crate had was
  its own test suite, which binds its own server. It now runs until interrupted.
- **A document the service created had no blocks.** An editor resolves a caret
  against a block, so a freshly created document that a client joined was a
  document nobody could type into. `create_document` now seeds the genesis with
  `Block::paragraph("")` — the model's own constructor, and the same thing
  `OpenDocApp::empty_titled` does for the same reason.

## What this needs from other crates, and does not do itself

Named rather than done, because `opendoc-app` and `opendoc-api` belong to other
workstreams:

- **A command for ingesting fanout.** If `apply_remote_operations` and its
  neighbours were commands in `opendoc-api`, both runtimes would reach them
  through the single `dispatch` entry point and the frame driver would exist
  once instead of twice. That is the right end state and it is an
  `opendoc-api` + `opendoc-app` change: a command whose argument is a batch of
  typed operations, plus `join_collaboration_session` and the presence and
  acknowledgement helpers beside it. Until then each runtime's transport drives
  the Rust methods directly, which is what a transport entry point is allowed
  to do (CLAUDE.md: "one transport entry point per runtime … plus native-only
  extras").
- **Nothing else.** Undo inside a live session *was* the second item here: it
  rewound `next_operation_seq` below the acknowledged watermark, and the service
  refuses to rewrite history. ADR 0017 fixed that properly while this was being
  built — an undo is now the inverse of this actor's own operations, authored
  like any other edit — and no workaround was implemented here. From a
  transport's side an undo is now simply more work to submit, which
  `an_undo_inside_a_live_session_is_submitted_as_new_work` asserts from outside
  the app: the batch that follows an undo carries sequence numbers *above*
  everything the service already holds.

  The guard both transports carry is kept as a cross-check rather than deleted.
  It fires when this replica holds fewer of its own operations than the service
  has made durable — the condition under which the next id it mints is one the
  log already holds — and it stops submitting and says so
  (`history-rewritten`). What it catches is silent divergence, and the
  alternative to catching it is not noticing. The batch watermark is
  `max(submitted on this connection, acknowledged by the service)` so that both
  ways of getting behind are covered.

## Consequences

- Collaboration is user-visible: a connection state, a role, an unsent count,
  who else is here, and a refusal in the service's own words.
- `opendoc-service` gains a browser-facing policy (origins) and keeps everything
  else it had. It is still absent from the WebAssembly graph.
- The wire format now has a generated, checked-in artefact. Changing the
  protocol means regenerating it, which is the same discipline the command
  contract already has.
- The browser and the native shell can now disagree about a bug: they share the
  protocol and the app, and not the transport. The fixture and the status field
  tests are what keep the disagreement small; the command named above is what
  would remove it.

## Known limitations

- **Two browser tabs on one origin still fight over storage** (ADR 0008). It
  does not affect this flow — a collaboration session's state is the app's and
  the service's, not the volume's — but it does mean two tabs must not both
  *save* the same repository. The browser end-to-end test therefore uses two
  Chrome instances, so two profiles and two independent stores, which is also
  the honest shape of "two people".
- **No TLS.** ADR 0015's limitation, unchanged: the native path refuses an
  `https://` address rather than quietly downgrading it, and the browser path
  will build a `wss://` URL if given one but the service cannot terminate it.
- **The native retry cap is not covered by an automated test.** The browser one
  is (`npm run e2e`); reaching it natively needs a service that accepts a
  connection and then stops answering, which an in-process graceful shutdown
  does not produce.
- **A page cannot tell a refused origin from an unreachable host.** The browser
  hides both, so the failure notice names the origin allowlist as the usual
  cause rather than claiming to know.
- **Nothing renders a remote selection range**, only a caret anchor as text.
- **The outbound pump is a timer.** A batch is sent within 250 ms of the
  keystroke that authored it rather than immediately.

## Amendment: how a session gets *out* of a bad state

Dated 2026-09-13. The four reconnect steps above are unchanged. What changes is
what puts a session into them, because until now almost nothing did: a refused
batch and a commit that would not apply both left the session in phase `live`
with `can_submit: false`, for ever.

### The outbox is chunked, to the service's number

Both transports called `local_operations_after(from)` and sent the result as
one `Submit`. A tick that found more than the service's cap — a replayed
disconnection, a large paste, an import — built a batch the service refused.

Both now chunk to `max_operations_per_submit` as the **welcome** stated it, at
most eight frames per tick so a replayed hour of typing reaches the service
steadily rather than as one burst. The cap is not restated on the client side
at all; ADR 0015's amendment says why.

### A refusal resynchronises, three times, and then stops

A refusal is a statement about a batch, so resubmitting the same batch is
pointless and the session still stops submitting the moment one arrives. But
*why* a batch is wrong is almost always that this replica and the service
disagree about what the service already holds — and the welcome settles that
from the service's own log, because step 3 re-derives the acknowledged
watermark from it. So a refusal now asks for a fresh connection.

Bounded, because a refusal a welcome cannot fix would otherwise be an infinite
reject-reconnect loop, which is the same lie as "Reconnecting…" against a
service that will never answer. Three refusals with no accepted batch between
them ends the session with the service's own words. An accepted batch resets
the count: it is the only evidence the two sides agree.

Only a refusal that arrives while the session is still submitting is counted. A
chunked outbox can have several batches in flight, and once the first is
refused the rest are the same failure arriving again.

### A commit that cannot be applied ends the connection

`apply_remote_operations` is all or nothing, so a failure loses the **whole**
commit. The browser recorded a notice and carried on; the native path did the
same **and advanced `commit_seq` for a commit it had not applied**, which made
the divergence permanently invisible.

Both now leave the sequence where it is — this replica really is behind it —
stop submitting, and ask for a fresh welcome. The welcome carries the base and
the whole log, so the lost commit comes back. That is the only re-request this
protocol has: there is no message for "send me commit *n* again", and adding
one would need the incremental welcome the protocol also does not have.

### An acknowledgement that never comes

There was no acknowledgement timeout anywhere. `submitted_through` moves as a
batch goes out and comes back only when the socket closes, so an `Accepted`
dropped on a socket that stayed open stranded that batch and everything after
it, with the pill reading "Live".

Both transports now roll the watermark back after forty ticks (ten seconds) of
silence, and the next tick sends the same operations again. Resending is safe
by construction rather than by hope: an operation id already in the service's
log may be resubmitted with a byte-identical payload, and the service
acknowledges that without committing anything (ADR 0015, "History
immutability"). The worst case of a timeout that fired early is one redundant
frame.

### How the browser is told, since it owns the socket

`CollabStatus` gains `reconnect_requested`, read once like `document_changed`.
`collab.ts` acts on it by calling the same `dropSocket` its harness hook calls,
which runs the production reconnect path. The native shell needs no field: it
owns its socket, so a resynchronisation is this connection ending with a
resumable notice and the loop in `NativeCollab::run` acting on it. The field is
present and always `false` on that side so `collab.ts` keeps one type.

### The order the browser composes and sends in

`collab_outbox()` advances Rust's watermark as it composes a frame, and
`collab.ts::flush` called it *before* checking whether the socket could take
one — so a `CLOSING` socket, or a `send` that threw, discarded frames Rust
believed had gone out, recoverable only if a `close` event happened to arrive.
`NativeCollab::sweep` always awaited its submit before moving its watermark.
That asymmetry was undocumented; it is now gone. `flush` asks for frames only
when the socket is `OPEN`, and a `send` that throws ends the connection, which
is also what rolls the watermark back.

### Three states that used to read "Live"

- **`unsendable-work`** — work this replica cannot replay onto the service's
  log. The notice always said `resumable: false`, but the phase stayed `Live`,
  so the pill read "Live" over a session that could never send again. Both
  paths now end the session. The document is still there and still saveable;
  the *session* is what is over. It had no test anywhere and now has one on
  each path.
- **A non-resumable notice followed by a dropped socket** left the browser on
  "Reconnecting…" with nothing retrying. `socket_closed` now reads the standing
  notice and says the session is over.
- **The native welcome never checked `protocol_version`.** The browser always
  did; `ServiceClient::connect` discarded the field with a `..`, so the one
  path that ships the server's own types beside it was the one that guessed. It
  is checked there now, and a mismatch is not resumable.

### What tests this

- `opendoc-wasm`'s `collab_tests` drive the browser driver over crafted frames:
  chunking (the exact frames and sequence numbers), a refusal recovering and a
  refusal giving up, an accepted batch forgiving earlier refusals, a commit that
  cannot be applied, the acknowledgement timeout and its absence.
- `opendoc-service`'s `app_client_tests` drive **two real clients over a real
  socket**: an oversized batch refused with the log unmoved and the same ids
  taken once chunked, a Commenter commenting and failing to type, revocation
  dropping sessions, the quota, the open-document limit, the anchor cap.
- `src-tauri`'s tests put a **cuttable TCP relay** in front of a real service,
  which is what the native path was missing: it owns its socket, so a test
  could not drop one. Cutting the relay aborts every connection in flight and
  refuses new ones, which is what an unplugged cable looks like from inside the
  process. The four reconnect steps and a chunked replay of a disconnection are
  both driven through it.

  This also removes the known limitation "the native retry cap is not covered
  by an automated test" from being unreachable — a cable that stays cut is
  exactly the service that accepts a connection and then stops answering.
