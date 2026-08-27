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

DBus is disabled in the prototype Tauri feature set because the app does not use tray/DBus integration.

## Current Scope

The app is a schema-surface prototype:

- renders paragraphs/headings
- renders list items, page breaks, and block equations
- renders links
- renders mentions and footnote references
- renders citation labels backed by the document-local citation database
- renders inline equations
- renders tables
- renders a sparse spreadsheet workbook with editable cells and deterministic `SUM` formula projection
- shows comments, suggestions, citation metadata, warnings, locale, and signature state
- creates a new blank document with fresh local state
- can add sample nodes through Tauri commands
- can edit text/link/mention/inline-equation nodes and block-equation source through operation commands
- can apply basic text marks to the focused inline
- can delete comment threads and accept/reject suggestions
- can edit spreadsheet cells through app operations; formula source is persisted and signed, computed values are projections
- saves and opens a document projection through the local content-addressed object repository
- commits saved snapshots and app-level operation journal segments through the manifest/head path used by `opendoc-store`
- signs the current canonical snapshot with a pasted OpenSSH private key
- persists multiple signature sidecar records as manifest-referenced objects
- verifies the current in-memory signature and clears signature state after edits

Full-fidelity operation-log persistence, real editor selection/IME handling, and production citation rendering are intentionally behind future API work.

## Browser Demo Backend

The TypeScript frontend uses `src/invoke.ts` instead of importing `@tauri-apps/api` directly. In Tauri it calls the global Tauri invoke API. Outside Tauri it uses an in-memory browser-demo backend that exercises the same rendering and button flow.

This keeps the UI source usable before all native Tauri packages are installed. The demo backend is not persistence/signing truth; the Rust commands remain authoritative for the desktop app.

The no-registry smoke check verifies that the frontend source has no external runtime import and that the mock backend covers the Tauri commands:

```sh
npm run verify
npm run smoke
npm run mock-contract
npm run gui-smoke
```

It also checks that every command in `commands.v0.json` is implemented by the Rust `#[tauri::command]` layer, registered in `tauri::generate_handler!`, covered by the reusable `opendoc-app-api` dispatcher, covered by the browser-demo mock backend, represented in `src/commands.ts`, called by the frontend with matching argument names, that `src/types.ts` matches the documented top-level app projection fields, that `dist/` exists, and that generated JavaScript is syntactically valid.

The GUI smoke check loads the generated browser bundle under a small fake DOM,
renders the full sample schema surface through the mock invoke backend, and
exercises representative document, mark, equation, spreadsheet, citation,
comment, suggestion, repository, signing, verification, and command-error
recovery flows.

The mock contract check calls every generated mock command and recursively
validates the returned `AppDocument` projection shape.
