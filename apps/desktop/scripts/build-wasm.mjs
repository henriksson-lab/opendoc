// Builds the Rust core to WebAssembly and generates the JS glue used by the
// browser build (src/wasm/). Requires the wasm32-unknown-unknown target and a
// wasm-bindgen CLI matching the wasm-bindgen crate version in Cargo.lock.
//
// The browser fetches this module on every cold load, so its size is a
// user-visible cost and every step here prints the size it produced. Two
// things shrink it, and neither changes what the module computes:
//
//   * the `release-wasm` cargo profile (workspace `Cargo.toml`): fat LTO and
//     one codegen unit, which only the browser build pays for;
//   * dropping the `name` custom section after wasm-bindgen — ~1.8 MB of Rust
//     symbol names that nothing in the browser reads. Set `WASM_KEEP_NAMES=1`
//     to keep them when a panic trace needs them.
//
// **`wasm-opt` is deliberately not run**, and that is a measurement rather
// than an omission. On top of this profile it takes the module from 15.27 MB
// to 14.65 MB at `-O2` (4.1%) or 14.29 MB at `-Oz` (6.4%) — and costs 20–45%
// more time in `layout_document` and a few percent in an editing command,
// measured through this module's own command surface. The core lays the
// document out on every edit, so that is the wrong side of the trade; fat LTO
// has already done the work binaryen would repeat. Leaving it out also keeps
// the artifact identical on every machine, which a build that silently used a
// tool if it happened to be installed would not. It is still worth re-running
// the comparison if the module's shape changes: `wasm-opt -O2 <module> -o
// <out>`, then time `dispatch("layout_document", "{}")` against both.
//
// The `name` section is dropped here, in a few lines of section walking,
// rather than with the cargo profile's `strip` setting, because `strip` also
// removes `target_features` — and a module without that section is one
// `wasm-opt`, `wasm2c` and every other post-processor rejects, believing its
// instructions are not enabled.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const desktopRoot = new URL("..", import.meta.url).pathname;
const repoRoot = join(desktopRoot, "../..");
const targetDir = process.env.CARGO_TARGET_DIR ?? join(repoRoot, "target");
const outDir = join(desktopRoot, "src/wasm");
/** The size-tuned profile in the workspace `Cargo.toml`. */
const profile = "release-wasm";

function run(command, args, options = {}) {
  console.log(`$ ${command} ${args.join(" ")}`);
  execFileSync(command, args, { stdio: "inherit", cwd: repoRoot, ...options });
}

function lockedWasmBindgenVersion() {
  const lock = readFileSync(join(repoRoot, "Cargo.lock"), "utf8");
  const match = lock.match(/name = "wasm-bindgen"\nversion = "([^"]+)"/);
  return match ? match[1] : null;
}

function megabytes(bytes) {
  return `${(bytes / 1_048_576).toFixed(2)} MB`;
}

/** A LEB128 unsigned integer at `offset`, and where it ends. */
function readVaruint(bytes, offset) {
  let value = 0;
  let shift = 0;
  let index = offset;
  for (;;) {
    const byte = bytes[index];
    index += 1;
    value += (byte & 0x7f) * 2 ** shift;
    if ((byte & 0x80) === 0) {
      return [value, index];
    }
    shift += 7;
  }
}

/**
 * Rewrites `path` without the named custom sections.
 *
 * A wasm module is a header and a flat list of length-prefixed sections, so
 * dropping one is a walk and a concatenation — no wasm library, and nothing
 * that could rewrite an instruction. The result is validated before it is
 * written: a module this step could not parse must never reach the browser.
 */
function dropCustomSections(path, drop) {
  const bytes = readFileSync(path);
  const kept = [bytes.subarray(0, 8)];
  let removed = 0;
  let index = 8;
  while (index < bytes.length) {
    const start = index;
    const id = bytes[index];
    index += 1;
    const [size, afterSize] = readVaruint(bytes, index);
    const body = afterSize;
    index = body + size;
    if (id === 0) {
      const [nameLength, afterName] = readVaruint(bytes, body);
      const name = bytes.subarray(afterName, afterName + nameLength).toString("utf8");
      if (drop.has(name)) {
        removed += index - start;
        continue;
      }
    }
    kept.push(bytes.subarray(start, index));
  }
  if (removed === 0) {
    return 0;
  }
  const stripped = Buffer.concat(kept);
  if (!WebAssembly.validate(stripped)) {
    throw new Error(`stripping custom sections from ${path} produced an invalid module`);
  }
  writeFileSync(path, stripped);
  return removed;
}

run("cargo", ["build", "-p", "opendoc-wasm", "--target", "wasm32-unknown-unknown", "--profile", profile], {
  env: { ...process.env, CARGO_TARGET_DIR: targetDir },
});
mkdirSync(outDir, { recursive: true });
const wasmFile = join(targetDir, `wasm32-unknown-unknown/${profile}/opendoc_wasm.wasm`);
if (!existsSync(wasmFile)) {
  throw new Error(`wasm artifact missing at ${wasmFile}`);
}
console.log(`cargo produced ${megabytes(statSync(wasmFile).size)}`);

const wanted = lockedWasmBindgenVersion();
try {
  run("wasm-bindgen", ["--target", "web", "--out-dir", outDir, "--out-name", "opendoc_wasm", wasmFile]);
} catch (error) {
  console.error(
    `wasm-bindgen failed. Install the CLI matching Cargo.lock: cargo install wasm-bindgen-cli --version ${wanted ?? "<see Cargo.lock>"}`,
  );
  throw error;
}

const bundled = join(outDir, "opendoc_wasm_bg.wasm");
console.log(`wasm-bindgen produced ${megabytes(statSync(bundled).size)}`);

if (process.env.WASM_KEEP_NAMES === "1") {
  console.log("WASM_KEEP_NAMES=1: keeping the symbol names for panic traces");
} else {
  const removed = dropCustomSections(bundled, new Set(["name", "producers"]));
  console.log(`without symbol names ${megabytes(statSync(bundled).size)} (${megabytes(removed)} of names dropped)`);
  // A current wasm-bindgen or Rust profile may already have omitted both
  // sections. That is the desired output, not a failure: `dropCustomSections`
  // has walked the complete module and would remove either section if present.
  // The count remains useful build telemetry without confusing an
  // already-stripped module for one that still ships symbol names.
}

console.log(`wasm glue written to ${outDir}`);
