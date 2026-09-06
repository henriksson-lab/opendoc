import { join } from "node:path";
import { run } from "./run-command.mjs";

const desktopRoot = new URL("..", import.meta.url).pathname;
const repoRoot = join(desktopRoot, "../..");

run("cargo fmt check", "cargo", ["fmt", "--all", "--check"], { cwd: repoRoot });
run("cargo clippy", "cargo", ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"], { cwd: repoRoot });
run("cargo test", "cargo", ["test", "--workspace"], { cwd: repoRoot });
run("cargo test opendal app api", "cargo", [
  "test",
  "-p",
  "opendoc-app-api",
  "--features",
  "opendal-store",
  "opendal_fs",
], { cwd: repoRoot });
run("desktop typecheck", "npm", ["run", "typecheck"], { cwd: desktopRoot });
run("desktop wasm build", "npm", ["run", "build:wasm"], { cwd: desktopRoot });
run("desktop build", "npm", ["run", "build"], { cwd: desktopRoot });
run("desktop smoke", "npm", ["run", "smoke"], { cwd: desktopRoot });

console.log("\nOpenDoc desktop verification passed");
