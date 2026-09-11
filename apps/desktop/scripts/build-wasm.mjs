// Builds the Rust core to WebAssembly and generates the JS glue used by the
// browser build (src/wasm/). Requires the wasm32-unknown-unknown target and a
// wasm-bindgen CLI matching the wasm-bindgen crate version in Cargo.lock.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const desktopRoot = new URL("..", import.meta.url).pathname;
const repoRoot = join(desktopRoot, "../..");
const targetDir = process.env.CARGO_TARGET_DIR ?? join(repoRoot, "target");
const outDir = join(desktopRoot, "src/wasm");

function run(command, args, options = {}) {
  console.log(`$ ${command} ${args.join(" ")}`);
  execFileSync(command, args, { stdio: "inherit", cwd: repoRoot, ...options });
}

function lockedWasmBindgenVersion() {
  const lock = readFileSync(join(repoRoot, "Cargo.lock"), "utf8");
  const match = lock.match(/name = "wasm-bindgen"\nversion = "([^"]+)"/);
  return match ? match[1] : null;
}

run("cargo", ["build", "-p", "opendoc-wasm", "--target", "wasm32-unknown-unknown", "--release"], {
  env: { ...process.env, CARGO_TARGET_DIR: targetDir },
});
mkdirSync(outDir, { recursive: true });
const wasmFile = join(targetDir, "wasm32-unknown-unknown/release/opendoc_wasm.wasm");
if (!existsSync(wasmFile)) {
  throw new Error(`wasm artifact missing at ${wasmFile}`);
}
const wanted = lockedWasmBindgenVersion();
try {
  run("wasm-bindgen", ["--target", "web", "--out-dir", outDir, "--out-name", "opendoc_wasm", wasmFile]);
} catch (error) {
  console.error(
    `wasm-bindgen failed. Install the CLI matching Cargo.lock: cargo install wasm-bindgen-cli --version ${wanted ?? "<see Cargo.lock>"}`,
  );
  throw error;
}
console.log(`wasm glue written to ${outDir}`);
