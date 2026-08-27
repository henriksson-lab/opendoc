# ADR 0001: Collaboration Core

Status: provisional decision.

## Context

OpenDoc needs collaborative editing, single-user editing, local offline editing, and raw object-storage persistence. Classic operational transformation can work for centralized Google Docs-style sessions, but offline-first storage and branchable history make CRDTs a better default to evaluate.

## Decision

Use an Automerge-style CRDT as the first collaboration core prototype. Keep the document schema independent enough that a Yjs bridge remains possible for web editor integration.

Batching is not part of the user-visible editing semantics. The editor renders from the live collaboration document after each local or remote operation. Operation segments and manifest commits are asynchronous persistence/versioning artifacts.

## Rationale

- Offline-first and raw object storage are primary requirements.
- Rich text needs marks, block markers, and stable anchors.
- Version history, signed snapshots, and compaction are easier to reason about with immutable CRDT changes plus checkpoints.
- Yjs remains important for frontend ecosystem comparison, but its JavaScript-first implementation should not own the Rust core by default.

## Consequences

- We must validate performance on large documents.
- We must define how spreadsheet row and column operations are represented.
- We need a clear projection layer from CRDT state to editor-specific schemas.
- Presence is explicitly out of the persisted CRDT and travels over a separate ephemeral channel.
- Rich-text formatting must be represented as mergeable marks anchored to stable elements, not plain index ranges.
- All core merges must be automatic; manual merge conflict resolution is not acceptable for normal document editing.

## Validation Required

- Concurrent rich-text edits converge.
- Overlapping marks survive concurrent changes.
- Concurrent mark add/remove converges deterministically.
- Local rendering happens before storage batching or manifest signing.
- Comment and citation anchors remain stable after insert/delete.
- Spreadsheet row insertion and formula reference updates converge.
