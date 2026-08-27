import { join } from "node:path";
import { run } from "./run-command.mjs";

const desktopRoot = new URL("..", import.meta.url).pathname;
const repoRoot = join(desktopRoot, "../..");

run("cargo fmt", "cargo", ["fmt"], { cwd: repoRoot });
run("cargo test", "cargo", ["test"], { cwd: repoRoot });
run("desktop build", "npm", ["run", "build"], { cwd: desktopRoot });
run("desktop smoke", "npm", ["run", "smoke"], { cwd: desktopRoot });
run("desktop mock contract", "npm", ["run", "mock-contract"], { cwd: desktopRoot });
run("desktop GUI smoke", "npm", ["run", "gui-smoke"], { cwd: desktopRoot });

console.log("\nrunnable OpenDoc desktop verification passed");
