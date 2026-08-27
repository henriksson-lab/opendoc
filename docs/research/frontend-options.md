# Frontend Options Research

Status: initial complete draft.

## Sources

- Leptos crate documentation: https://docs.rs/leptos/latest/leptos/
- Tauri webview documentation: https://docs.rs/tauri/latest/tauri/webview/index.html
- ProseMirror guide: https://prosemirror.net/docs/guide/

## Findings

Leptos is a Rust web framework that can support browser, server-rendered, and hydrated application shapes. Tauri provides a Rust desktop shell around a webview and is a strong fit for local filesystem, offline, and signing workflows. ProseMirror is not Rust-native, but its schema, transaction, plugin, and collaborative editing concepts are mature enough to make it the likely first serious rich-text editing surface.

## Candidate Routes

| Route | Strength | Weakness | Recommendation |
| --- | --- | --- | --- |
| Leptos web app | Rust/WASM alignment, deployable in browser | editor ecosystem less mature than ProseMirror | good for app shell and Rust-heavy UI |
| Tauri app | strong local filesystem/signing integration | desktop packaging and webview behavior | likely best first offline app |
| Shared Rust core plus web editor | keeps model in Rust, uses mature JS editor | boundary complexity | recommended architecture |
| Pure JS editor/core | fastest editor integration | violates Rust-first core goal | reject for core |

## Editor Surface

The canonical document state belongs to the Rust core/collaboration layer. The frontend:

- renders projections
- emits intent-level edits
- displays presence
- handles IME, selection, and accessibility concerns
- never becomes the signed source of truth

## Recommendation

Use a shared Rust core with two possible shells:

- Tauri for local/offline-first desktop workflows.
- Web/Leptos for browser deployments and collaboration UI.

For rich text, evaluate ProseMirror/Tiptap as the first serious editor surface because schema constraints, plugins, decorations, and collaboration integrations are mature. For spreadsheets, expect a custom grid surface backed by the Rust model.

## Rejected Options

- **Pure frontend-owned state.** Rejected because signed storage and offline sync need a canonical Rust core.
- **Landing-page-first web app.** Rejected because this project needs a usable editor surface, not marketing.
- **Native-only desktop UI.** Rejected for now because rich-text editor ecosystems are much stronger in web technology.

## Offline Workflow

1. Create local document.
2. Apply edits to local CRDT/document core.
3. Write operation segments and snapshots to local object layout.
4. Sign manifests locally.
5. Later connect to S3 or server.
6. Upload missing immutable objects.
7. Reconcile branch head conflicts.
8. Verify signed versions after sync.

## Open Risks

- WASM boundary cost for large documents.
- Keeping editor selections stable during projection updates.
- Spreadsheet accessibility and performance.

## Prototype Evidence

The current prototypes are CLI-level model prototypes. UI spikes are still required before frontend selection is final.
