# OpenDoc Desktop

Prototype Tauri shell for the hybrid TypeScript frontend plus Rust core API.

## Layout

- `src/`: TypeScript UI.
- `src/types.ts`: compatibility barrel over generated Rust-owned projection types plus frontend-only runtime config.
- `src/commands.ts`: shared TypeScript command argument/result types for the UI and invoke layer.
- `src-tauri/`: Tauri backend and command handlers.
- `../../crates/opendoc-app`: runtime-facing Rust app facade used by Tauri and WASM.

## Run the native shell

The Tauri app works: it builds, launches, opens and edits documents, and
bundles. Both entry points are verified.

```sh
npm install
npm run preflight        # are the GTK/WebKit pkg-config files here?
npm run native-check     # + icon check, command-ACL check, cargo check --release
npm run tauri dev        # vite on 10084 + cargo run (add -- --release; see below)
npm run tauri build      # deb + rpm + AppImage in src-tauri/target/release/bundle/
```

`npm run tauri dev` runs `beforeDevCommand` (`npm run dev`, vite on
`0.0.0.0:10084` with `strictPort`) and points the webview at
`http://127.0.0.1:10084`. Two consequences worth knowing:

- **Something else on 10084 breaks it.** vite exits, and the shell then loads
  whatever *is* on that port — or shows "Could not connect to 127.0.0.1". To
  run a second instance, override both halves rather than editing the config:
  `npm run tauri -- dev --config '{"build":{"beforeDevCommand":"npx vite --host 127.0.0.1 --port 10085 --strictPort","devUrl":"http://127.0.0.1:10085"}}'`
- **`tauri dev` defaults to a debug cargo profile**, which this workspace has
  no use for. Pass `npm run tauri -- dev --release`; it then shares the target
  directory with every other step here.

`npm run tauri build` runs `beforeBuildCommand` (`npm run build:app` =
`build:wasm` + `build`), compiles release, and bundles. `bundle.targets` is
`"all"`, so on Linux that is deb, rpm and AppImage; the AppImage step downloads
`linuxdeploy` on first use, so that one build needs network access.

The built binary is *not* the same as a plain `cargo build --release` in
`src-tauri`: only the bundling path defines Tauri's `custom-protocol`, so only
it serves the embedded `dist/`. A hand-rolled `cargo build --release` binary
still expects the dev server on `devUrl`. Use `npm run tauri build` (or
`cargo build --release --features tauri/custom-protocol`) when you want a
standalone app, and treat `cargo check --release` as a compile check only.

`npm run build` deliberately needs no npm registry and no bundler: it uses
`scripts/build.mjs` to emit a static `dist/` from the TypeScript source, so the
WASM/browser path stays testable even where the Tauri prerequisites are absent.

### Linux prerequisites

These really are required — `cargo check` fails without them, with a
`pkg-config` error rather than anything about Tauri:

- Ubuntu/Debian: `libgtk-3-dev libwebkit2gtk-4.1-dev pkg-config`
- Fedora: `gtk3-devel webkit2gtk4.1-devel pkgconf-pkg-config`

`npm run preflight` checks for `gdk-3.0`, `webkit2gtk-4.1`,
`javascriptcoregtk-4.1` and `libsoup-3.0` and names the package to install for
each. The Tauri feature set here is `wry`, `compression`,
`common-controls-v6`, `dynamic-acl` and `x11` with default features off: no
tray and no DBus integration (the app uses neither), and **X11 only**, so under
Wayland it runs through XWayland.

Two optional host tools widen what the shell can do rather than whether it
runs: `pandoc` **or** LibreOffice for `.doc`/`.docx` import, and outbound
network access for insert-image-by-URL.

`.github/workflows/desktop.yml` is the Linux CI gate: it installs the system
packages, then runs `npm run verify` and `npm run native-check`.

### Adding a native command

A command has to be named in three places and the compiler only enforces one:

1. `src-tauri/src/main.rs` — `#[tauri::command]` plus `generate_handler![…]`.
2. `src-tauri/build.rs` — the `COMMANDS` list, which is what mints the
   `allow-…`/`deny-…` ACL permissions for it.
3. `src-tauri/capabilities/default.json` — the `allow-<kebab-case-name>` grant.

Miss either of the last two and the command compiles, links, and is then
refused at runtime by the ACL with nothing in the frontend able to predict it.
That is not hypothetical: `fetch_url_base64` (insert image by URL) shipped that
way and was dead in the native shell while `cargo check` stayed green.
`npm run native-check` now cross-checks the three lists and fails on any
mismatch, which is the only automated place this can be caught — no Rust test
and no browser test sees the ACL.

`npm run native-check` also verifies that `src-tauri/icons/icon.png` exists and
is a PNG before `cargo check` reaches `tauri::generate_context!()`.

### Icons

`src-tauri/icons/` holds the standard Tauri set (`32x32.png`, `128x128.png`,
`128x128@2x.png`, `icon.icns`, `icon.ico`, the Windows `Square*Logo.png`), and
`tauri.conf.json` lists them under `bundle.icon`. Both parts matter: without
the `bundle.icon` list the deb and rpm install no desktop icon at all and the
AppImage bundler aborts with `couldn't find a square icon to use as AppImage
icon`. Regenerate from a square source with
`npx tauri icon path/to/1024.png` (then delete the `android/` and `ios/`
subdirectories it also writes — there are no mobile targets here).

### Debugging the native shell

The webview has no inspector in a release build. Build with the opt-in feature
and point WebKit's remote inspector at a port:

```sh
cargo build --release --features devtools
WEBKIT_INSPECTOR_SERVER=127.0.0.1:9223 ./target/release/opendoc-desktop
```

Connect from another WebKit-based browser (`inspector://127.0.0.1:9223`) —
WebKitGTK's inspector server does not speak HTTP or the Chrome DevTools
protocol, so Chrome-based tooling and `curl` get an empty reply.

### Running it headless

`DISPLAY`-less machines can still exercise the app rather than only compile it:

```sh
Xvfb :99 -screen 0 1400x1000x24 &
DISPLAY=:99 WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 \
  LIBGL_ALWAYS_SOFTWARE=1 ./src-tauri/target/release/bundle/appimage/OpenDoc_*.AppImage
DISPLAY=:99 import -window root shot.png    # ImageMagick
```

Set `XDG_DATA_HOME` to a scratch directory to keep the run's state out of the
real one; the crash-recovery journal lives at
`$XDG_DATA_HOME/org.opendoc.prototype/recovery/*.recovery`. There is no window
manager under a bare Xvfb, so a dialog will not be raised or focused for you —
`XRaiseWindow`/`XSetInputFocus` it first, and send `WM_DELETE_WINDOW` yourself
to test the close guard.

## What only the native shell can do

These paths exist nowhere else — the browser build either has no such
capability or substitutes a DOM stand-in — so `npm run verify` and the jsdom
smoke test cannot see them. All of them were driven through the real UI of a
bundled AppImage under Xvfb:

| Path | Commands | Verified by |
| --- | --- | --- |
| Native open dialog | `pick_open_path` (file and directory, extension-filtered) | File ▸ Open folder…, Import Word…, Insert ▸ Image… |
| Native save dialog | `pick_save_path` | File ▸ Download as … |
| Filesystem read | `read_file_base64` | Insert ▸ Image… inserted a PNG off disk as a content-addressed blob |
| Filesystem write | `write_file_text`, `write_file_base64` | HTML export, and a 1-page PDF whose text `pdftotext` reads back |
| Repository round-trip | `dispatch` → `opendoc-store` | Save wrote `objects/`, `documents/`, `indexes/`; Open folder read the document back |
| `.doc`/`.docx` import | `import_doc_or_docx_path` | a pandoc-made `.docx` imported with its heading and paragraph |
| Insert image by URL | `fetch_url_base64` | fetched a public PNG, stored it as a blob, inserted the block |
| OS window title | `set_window_title` | title tracks the document name |
| Close guard | `WindowEvent::CloseRequested` → `opendoc://close-requested` → `close_window` | `WM_DELETE_WINDOW` with unsaved work is refused and prompts; "Discard changes and close" exits |
| Crash recovery (ADR 0005) | `FileRecoveryJournalStore` | `kill -9` mid-edit, then relaunch offered and replayed the 30 unsaved operations |

The recovery journal is installed only by this shell, from `setup()`, at
`$XDG_DATA_HOME/org.opendoc.prototype/recovery`. It holds **unsaved** work
only: saving (or the 5-second autosave, once a repository root exists) clears
the live segment, so an empty `recovery/` directory after a save is the
designed state and not a failure.

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

## Browser WASM Runtime

The TypeScript frontend uses `src/invoke.ts` instead of importing
`@tauri-apps/api` directly. In Tauri it calls the global Tauri invoke API.
Outside Tauri it loads `src/wasm/opendoc_wasm.js`, which dispatches to the same
Rust `OpenDocApp` command surface compiled to WebAssembly.

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

Build the browser WASM adapter before building the static frontend:

```sh
npm run build:wasm
npm run build
npm run smoke
```

`npm run verify` includes the workspace Rust tests and the feature-gated
OpenDAL filesystem app tests:
`cargo test --release -p opendoc-app --features opendal-store`.

It also runs `cargo fmt --check`, clippy with warnings denied, the generated
command-contract drift check, TypeScript type checking, the WASM build, the
static frontend build, and the jsdom smoke test against the real Rust/WASM
dispatcher.

**Every cargo step uses `--release`.** Debug builds of this workspace are slow
and give nothing back, and keeping one profile means the steps share a target
directory instead of building the tree twice.

`.github/workflows/desktop.yml` is the Linux CI gate: it installs the GTK/WebKit
system packages, then runs `npm run verify` and `npm run native-check`. Nothing
asserts the workflow's own contents, so changes to it are only caught by CI
running.
