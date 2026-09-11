# Tauri And Browser Architecture Decisions

If OpenDoc targets Tauri while still allowing browser use, the main decision is to keep document logic platform-neutral and make Tauri only one shell around it.

## 1. Core Runtime Boundary

Put document state, merge, signing, storage formats, citations, spreadsheet logic, and import/export in Rust crates that do not depend on Tauri.

Suggested split:

- `opendoc-core`: schema/state
- `opendoc-merge`: operation merge
- `opendoc-format`: binary encoding
- `opendoc-store`: storage trait
- `opendoc-sign`: signatures
- `opendoc-citations`: citum adapter
- `opendoc-app-api`: stable API used by UI
- `apps/tauri`: local desktop shell
- `apps/web`: browser shell

Tauri should call Rust through commands/message passing.

## 2. Browser Vs Tauri Storage

Use one storage abstraction with multiple backends:

- Tauri/local app:
  - local filesystem object repo
  - later S3 via OpenDAL
  - optional OS keychain integration
- Browser:
  - IndexedDB/object storage
  - remote S3 only if credentials/CORS are workable
  - no direct local disk except file picker/export/import

The online browser version has two deployment flavors:

- Per-user HPC node:
  - runs on a node at the HPC facility, likely behind an existing authenticated access path such as Open OnDemand
  - can access local disk and S3-like storage
  - handles one user at a time
  - assumes authentication is already done
  - treats the connected user as fully authorized for reachable files/objects
  - does not need document-level permission checks
- Continuously running multi-user service:
  - runs as a shared service outside the Open OnDemand-style trust boundary
  - accepts connections from multiple users
  - must own authentication
  - must own document permissions and sharing
  - must protect object lookup, branch heads, signatures, comments, suggestions, and blobs by access policy

Decision: `ObjectStore` remains the boundary. Keep file IO in Rust backend for consistency and signing safety.

Implication: storage code should be independent of identity, but repository-opening code should receive a capability/context:

- local/Tauri context: path or bucket credentials selected by the user
- HPC single-user context: server-side filesystem/S3 capability for the authenticated user
- multi-user service context: authenticated principal plus permission-checked repository capability

## 3. Frontend Framework

Decision: use a hybrid TypeScript frontend with Rust document logic behind a stable API.

Leptos remains possible for specific Rust/WASM UI experiments, but it is not the default frontend direction. The rich-text editor surface, selection handling, IME behavior, DOM integration, and user interaction layer should be TypeScript.

Options:

- Leptos + Rust/WASM editor model:
  - better code sharing
  - harder rich-text DOM/editor ergonomics
- TypeScript editor surface + Rust core through WASM/Tauri commands:
  - easier browser editing integration
  - more duplicated type bindings
- Hybrid:
  - Rust owns document operations
  - frontend owns selection, IME, rendering, input events
  - operations cross boundary as binary/typed messages

Chosen model: hybrid TypeScript frontend plus Rust core.

## 4. Shared API Contract

Define a stable app-facing API independent of Tauri:

- open repo/document
- read current projection
- apply local operation
- receive merged remote operations
- commit snapshot
- render citation caches
- sign version
- verify version
- list warnings

This API should exist as Rust traits/functions first. Tauri commands and WASM bindings wrap it.

## 5. Binary Format In Browser

If browser code calls Rust via WASM, it can use the same CBOR/object formats.

Decision:

- Internal storage/commit format: binary CBOR.
- UI API during early prototype: structured objects for debuggability.
- Add binary message path later for performance.

## 6. Signing And Keys

Tauri can read OpenSSH private keys from disk. Browser generally cannot, except imported keys or WebCrypto-backed keys.

Decision:

- Tauri: Rust-native OpenSSH key loading/signing.
- Browser: postpone signing implementation.
- Browser verification is likely useful later, but not a v0 commitment.
- Do not decide yet between imported key material, WebCrypto-backed keys, SSH-compatible browser adapters, or server-assisted signing.
- Never require signing to open documents.

## 7. Import/Export

Tauri app can bundle or call local converters. Browser cannot safely rely on LibreOffice/antiword/etc.

Decision:

- Tauri gets first-class `.doc/.docx` import path.
- Browser supports import only where pure Rust/WASM or upload-to-service exists.
- Keep import as adapter producing operations/document state.

## 8. Concurrency Model

Tauri local app may be single process, but browser and later S3 need the same commit semantics.

Decision:

- Always use operation log + manifest commits.
- Tauri local mode still uses CAS head updates.
- Multi-user behavior tested through simulations before server exists.

## 9. Editor Rendering

The renderer must not be the source of truth.

Decision:

- UI renders a projection of `Document`.
- Input creates operations.
- Merge updates state.
- UI reprojects state.
- Citation labels, comments, suggestions, and equations are special nodes in the projection, not links or plain text.

## 10. Immediate Build Decision

Do not scaffold Tauri yet. First create `opendoc-app-api` and maybe `opendoc-citations`.

Done when:

- the same API can be called from a CLI test,
- later from Tauri commands,
- later from WASM/browser bindings,
- without changing core document/version-control code.
