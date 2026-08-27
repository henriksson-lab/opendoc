import { join } from "node:path";
import { run } from "./run-command.mjs";

const desktopRoot = new URL("..", import.meta.url).pathname;
const tauriRoot = join(desktopRoot, "src-tauri");

run("desktop native preflight", "npm", ["run", "preflight"], { cwd: desktopRoot });
run("Tauri backend cargo check", "cargo", ["check"], { cwd: tauriRoot });

console.log("\nOpenDoc Tauri native check passed");
