// Release builds only: debug builds of this workspace are slow and give
// nothing back, so every cargo step here uses --release. Keeping one
// profile also means these steps share a target dir instead of building
// the tree twice.
import { join } from "node:path";
import { run } from "./run-command.mjs";

const desktopRoot = new URL("..", import.meta.url).pathname;
const repoRoot = join(desktopRoot, "../..");

run("cargo fmt check", "cargo", ["fmt", "--all", "--check"], { cwd: repoRoot });
run("cargo clippy", "cargo", ["clippy", "--release", "--workspace", "--all-targets", "--", "-D", "warnings"], { cwd: repoRoot });
run("cargo test", "cargo", ["test", "--release", "--workspace"], { cwd: repoRoot });
run("cargo test opendal app", "cargo", [
  "test",
  "--release",
  "-p",
  "opendoc-app",
  "--features",
  "opendal-store",
], { cwd: repoRoot });
run("desktop generated command bindings", "npm", ["run", "generate:commands", "--", "--check"], { cwd: desktopRoot });
run("desktop typecheck", "npm", ["run", "typecheck"], { cwd: desktopRoot });
run("desktop wasm build", "npm", ["run", "build:wasm"], { cwd: desktopRoot });
run("desktop build", "npm", ["run", "build"], { cwd: desktopRoot });
run("desktop smoke", "npm", ["run", "smoke"], { cwd: desktopRoot });
run("desktop e2e (real browser)", "npm", ["run", "e2e"], { cwd: desktopRoot });

console.log("\nOpenDoc desktop verification passed");
