# OpenDoc Desktop

Prototype Tauri shell for the hybrid TypeScript frontend plus Rust core API.

## Layout

- `src/`: TypeScript UI.
- `src/types.ts`: shared TypeScript projection types for the UI and browser-demo backend.
- `src/commands.ts`: shared TypeScript command argument/result types for the UI and invoke layer.
- `src-tauri/`: Tauri backend and command handlers.
- `../../crates/opendoc-app-api`: app-facing Rust API used by the Tauri backend.

## Run

```sh
npm install
npm run tauri dev
```

`npm run dev` serves the browser/Tauri frontend on `0.0.0.0:10084` by
default so the prototype can be tested from another machine or forwarded
session. The Tauri shell uses `http://127.0.0.1:10084` for its local dev URL.

For this research prototype, `npm run build` does not require installed npm packages. It uses `scripts/build.mjs` to emit a static `dist/` bundle from the TypeScript source so the Tauri shell has frontend assets even when the registry is unavailable.

On Linux, Tauri requires native WebView/system packages. This environment currently lacks the GTK/WebKit pkg-config files needed by Tauri, including `gdk-3.0.pc`, `javascriptcoregtk-4.1.pc`, and `libsoup-3.0.pc`; install the platform equivalent before building the Tauri backend:

- Ubuntu/Debian: `libgtk-3-dev libwebkit2gtk-4.1-dev pkg-config`
- Fedora: `gtk3-devel webkit2gtk4.1-devel pkgconf-pkg-config`

Check the local machine before running the native Tauri build:

```sh
npm run preflight
npm run native-check
```

The repository CI workflow at `.github/workflows/desktop.yml` installs these
Linux packages on Ubuntu and runs both `npm run verify` and
`npm run native-check`. That is the native build gate for environments where
the host WebKit/GTK packages are available.

`npm run native-check` also verifies that the required Tauri icon asset exists
at `src-tauri/icons/icon.png` and is a PNG before `cargo check` reaches
`tauri::generate_context!()`.

DBus is disabled in the prototype Tauri feature set because the app does not use tray/DBus integration.

## Current Scope

The app is a schema-surface prototype:

- renders paragraphs/headings
- renders list items, page breaks, and block equations
- renders links
- renders mentions and footnote references
- renders citation labels, including locator labels such as pages, backed by the document-local citation database and supports bibliography/citation delete flows
- renders inline equations
- renders tables
- renders a sparse multi-sheet spreadsheet workbook with editable cells, deterministic formula projection, and formula-error warnings
- shows comments, suggestions, citation metadata, warnings, locale, and signature state
- creates a new blank document with fresh local state
- can add sample nodes through Tauri commands
- can insert paragraphs after stable top-level blocks through operation-backed Tauri/browser commands
- can delete document blocks through operation-backed Tauri/browser commands
- keeps toolbar selection mapped to a live editable inline after command
  re-renders and clears stale selections after deletion or source replacement
- can update heading levels through operation-backed Tauri/browser commands
- can update list item nesting and ordered/unordered state through operation-backed Tauri/browser commands
- can insert text inline atoms through operation-backed Tauri/browser commands
- can delete selected inline atoms through operation-backed Tauri/browser commands
- can update selected link targets through operation-backed Tauri/browser commands
- can edit text/link/mention/inline-equation nodes and block-equation source through operation commands
- can apply and remove basic text marks on the focused inline
- can delete comment threads and accept/reject suggestions
- defers contenteditable commits until IME composition ends for document title,
  rich text, equations, comments, suggestions, footnotes, image alt text, sheet
  titles, spreadsheet cells, and spreadsheet cell comments
- can edit spreadsheet cells through app operations; formula source is persisted and signed, computed values are projections
- can add spreadsheet sheets and edit cells by stable sheet ID
- exposes stable row and column axis IDs for spreadsheet merge anchors while keeping display labels simple
- can apply signed source-level spreadsheet cell formatting through the same command contract
- derives a deterministic spreadsheet dependency graph and invalidation order from formula source
- can copy spreadsheet ranges within a sheet while preserving formatting and shifting relative formula references
- can define signed workbook-level named ranges and use them in formulas such as `SUM(QTY)`
- keeps spreadsheet protected ranges warning-only in v0, downgrading enforced-protection inputs with visible warnings
- imports constrained Google Docs API-shaped JSON into the OpenDoc schema through the same command contract used by Tauri
- imports `.doc` and `.docx` paths in Tauri/local mode through the Rust converter adapter when `pandoc` or LibreOffice is available
- exports the current v0 subset to Google Docs-like JSON for compatibility testing
- imports and exports constrained Google Sheets API-shaped workbooks with source formulas, basic formats, and named ranges
- attaches content-addressed binary blobs and shows their manifest availability state
- updates attachment display names and media types while preserving blob hash sidecar signatures
- signs attached binary blobs with detached exact-byte signature sidecars keyed by blob hash
- stores optional document DOI aliases and can reopen saved documents through DOI lookup records
- saves and opens a document projection through the local content-addressed object repository
- closes the current document into an explicit closed state while preserving recent documents for reopen
- commits saved snapshots and typed replayable app operation segments through the manifest/head path used by `opendoc-store`
- verifies the OpenDAL filesystem-backed repository path through feature-gated Rust tests in `npm run verify`
- signs the current canonical snapshot with a pasted OpenSSH private key
- persists multiple signature sidecar records as manifest-referenced objects
- verifies the current in-memory signature and clears signature state after edits

Full production selection mapping and production citation rendering are intentionally behind future API work.

## Browser Demo Backend

The TypeScript frontend uses `src/invoke.ts` instead of importing `@tauri-apps/api` directly. In Tauri it calls the global Tauri invoke API. Outside Tauri it uses an in-memory browser-demo backend that exercises the same rendering and button flow.

This keeps the UI source usable before all native Tauri packages are installed. The demo backend is not persistence/signing truth; the Rust commands remain authoritative for the desktop app.

Web deployments can set `window.__OPENDOC_RUNTIME__` before loading the
frontend bundle to identify the shell flavour without changing the command
contract or document format. Supported modes are `browser-local`,
`hpc-single-user`, and `multi-user-service`; Tauri is detected automatically as
`tauri-local`. Permissions are displayed as enabled only for
`multi-user-service` unless the host config explicitly overrides that flag.
The same host config can provide `defaultRepositoryRoot`,
`defaultFlatNamespace`, `storageBackends`, and `signingEnabled` so local, HPC,
and service shells can present appropriate defaults and visible capability
warnings while still using the same command names and source schema.

The no-registry smoke check verifies that the frontend source has no external runtime import and that the mock backend covers the Tauri commands:

```sh
npm run verify
npm run smoke
npm run runtime-contract
npm run runtime-capability-smoke
npm run workflow-check
npm run mock-contract
npm run gui-smoke
```

`npm run verify` includes the default workspace Rust tests and the
feature-gated OpenDAL filesystem app API tests:
`cargo test -p opendoc-app-api --features opendal-store opendal_fs`.

It also checks that every command in `commands.v0.json` is implemented by the Rust `#[tauri::command]` layer, registered in `tauri::generate_handler!`, listed in the Tauri build manifest, covered by generated Tauri allow/deny permission files and the default capability allow list, covered by the reusable `opendoc-app-api` dispatcher, covered by the browser-demo mock backend, represented in `src/commands.ts`, called by the frontend with matching argument names, that `src/types.ts` matches the documented top-level app projection fields, that `dist/` exists, and that generated JavaScript is syntactically valid.

The GUI smoke check loads the generated browser bundle under a small fake DOM,
renders the full sample schema surface through the mock invoke backend, and
exercises representative document, mark, equation, spreadsheet, citation,
comment, suggestion, Google Docs JSON import/export, attachment and attachment
signing, DOI lookup, repository, signing, verification, and command-error
recovery flows.

The runtime capability smoke check loads the same bundle with a browser-local
host config that lacks local storage and private-key signing, then verifies
visible warnings and graceful command failures while flat storage remains
available.

The workflow check verifies that `.github/workflows/desktop.yml` keeps the
Linux desktop CI gate wired to the no-registry desktop verification command,
the native Tauri backend check, and the required GTK/WebKit system packages.

The mock contract check calls every generated mock command and recursively
validates the returned `AppDocument` projection shape.
