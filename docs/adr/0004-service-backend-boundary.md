# ADR 0004: Service Backend Boundary

Status: accepted for restructure.

## Context

OpenDoc currently has two in-process runtimes: Tauri owns a local
`OpenDocApp` behind a mutex, and WASM owns a local `OpenDocApp` behind a
browser adapter. Both call the same Rust command surface, which is the right
direction for a local-first application.

The codebase also exposes runtime/session/share/sync command DTOs. Those DTOs
describe a future service backend, but the current `relay_runtime_sync` path
does not apply a durable multi-user protocol. It classifies operation envelopes
and reports accepted/deferred/rejected IDs. Treating that as a backend would
hide the fact that there is no authenticated network service, no durable
server-side operation log, and no server-owned permission enforcement.

## Decision

Do not create `opendoc-service` until the local operation, storage, merge, and
API boundaries are stable enough to expose over a network.

For now, Tauri and WASM remain thin local runtimes over the same Rust app/API
facade. Service-shaped DTOs may remain in `opendoc-api` as contract design and
UI integration points, but they must be documented as local simulations unless
they are backed by an actual service process.

When created, `opendoc-service` owns only network/service concerns:

- authenticated HTTP/WebSocket transport
- subject/session authentication
- server-enforced permissions
- presence fanout
- durable operation intake
- branch head serialization or candidate-head reconciliation
- document UUID/DOI lookup over service-visible indexes
- service audit events

It must call the same typed operation and storage layers as local runtimes. It
must not own document semantics, spreadsheet semantics, import/export logic,
rendering, signing primitives, or a separate command schema.

## Rationale

- Local-first editing and raw object storage are core product requirements, so
  service code cannot become the only real backend.
- Network transport should not define document correctness. The source model,
  operation validation, merge behavior, and storage integrity checks must be
  correct before a service exposes them.
- Runtime policy belongs outside document source state. Permissions, presence,
  sessions, and share invites are deployment concerns, not document records.
- A fake service in the app facade would preserve the current unclear backend
  state and make later synchronization bugs harder to isolate.

## Service Creation Gate

`opendoc-service` may be created after these conditions are true:

- command parsing is typed and generated or owned from `opendoc-api`
- app behavior is split into services behind a small facade
- operation application validates preconditions and postconditions
- merge tests cover small multi-actor streams
- storage tests cover local and object-store-like backends through one trait
- runtime/session/permission DTOs are owned by `opendoc-api`
- relay semantics are specified as operation intake, candidate commit, or both

## Consequences

- Current UI labels and docs must avoid implying that sync relay is a real
  server.
- Any future service prototype starts from storage and operation traits, not by
  copying `OpenDocApp` into a server crate.
- Permissions in Tauri/WASM remain advisory until a service enforces them.
- Presence remains ephemeral and outside signed document state.
- Service integration tests must prove replay/reopen behavior through durable
  storage, not just JSON request/response shape.

## Validation Required

- Tauri and WASM continue to call the same Rust command/API facade.
- A service-mode command cannot mutate a document without passing through the
  same typed operation validation used locally.
- Permission-denied service requests leave source state and branch heads
  unchanged.
- Accepted service operations survive restart and reopen from storage.
- Conflicting branch-head updates produce deterministic candidate-head recovery
  instead of silent last-writer-wins.
