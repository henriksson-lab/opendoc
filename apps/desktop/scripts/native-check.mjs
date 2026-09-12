import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { run } from "./run-command.mjs";

const desktopRoot = new URL("..", import.meta.url).pathname;
const tauriRoot = join(desktopRoot, "src-tauri");
const tauriIcon = join(tauriRoot, "icons", "icon.png");

checkTauriIcon();

run("desktop native preflight", "npm", ["run", "preflight"], { cwd: desktopRoot });
run("Tauri backend cargo check", "cargo", ["check", "--release"], { cwd: tauriRoot });

console.log("\nOpenDoc Tauri native check passed");

function checkTauriIcon() {
  if (!existsSync(tauriIcon)) {
    console.error("desktop native asset check failed");
    console.error(`Missing required Tauri icon: ${tauriIcon}`);
    console.error("Tauri generate_context! expects src-tauri/icons/icon.png to exist.");
    process.exit(1);
  }

  const header = readFileSync(tauriIcon).subarray(0, 8);
  const pngHeader = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  if (!header.equals(pngHeader)) {
    console.error("desktop native asset check failed");
    console.error(`Required Tauri icon is not a PNG file: ${tauriIcon}`);
    process.exit(1);
  }
}
