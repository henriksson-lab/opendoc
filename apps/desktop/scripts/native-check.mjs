import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { run } from "./run-command.mjs";

const desktopRoot = new URL("..", import.meta.url).pathname;
const tauriRoot = join(desktopRoot, "src-tauri");
const tauriIcon = join(tauriRoot, "icons", "icon.png");

checkTauriIcon();
checkCommandAcl();

run("desktop native preflight", "npm", ["run", "preflight"], { cwd: desktopRoot });
// `src-tauri` is `exclude`d from the workspace (root `Cargo.toml`), so
// `cargo clippy --workspace` and `cargo test --workspace` never reach it.
// Until these two lines existed its tests had never been run by any gate and
// nothing lint-checked the native shell; CI's `cargo check` did not even
// type-check the test module. `--all-targets` is what makes clippy compile
// the tests, and the test run is what makes them count. PLAN88 §7.
run("Tauri backend clippy", "cargo", ["clippy", "--release", "--all-targets", "--", "-D", "warnings"], {
  cwd: tauriRoot,
});
run("Tauri backend tests", "cargo", ["test", "--release"], { cwd: tauriRoot });

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

/**
 * Every native command has to be named in three places, and the compiler only
 * enforces one of them.
 *
 * `generate_handler!` makes a command reachable over IPC, `build.rs` mints the
 * `allow-…`/`deny-…` ACL permissions for it, and the window capability grants
 * one. A command missing from either of the last two compiles, links and then
 * fails at runtime with a denied-by-ACL rejection the frontend has no way to
 * predict — which is exactly how `fetch_url_base64` (insert image by URL) was
 * dead in the native shell while `cargo check` stayed green. This check is the
 * tripwire for that, since no Rust test and no browser test can see it.
 */
function checkCommandAcl() {
  console.log("\n== Tauri command ACL check ==");
  const handler = commandsInGenerateHandler(readFileSync(join(tauriRoot, "src", "main.rs"), "utf8"));
  const manifest = commandsInBuildScript(readFileSync(join(tauriRoot, "build.rs"), "utf8"));
  const capabilityPath = join(tauriRoot, "capabilities", "default.json");
  const granted = new Set(JSON.parse(readFileSync(capabilityPath, "utf8")).permissions ?? []);

  // A check whose two lists are empty reports "0 native commands are declared,
  // permitted and granted" and exits 0 — both loops below are no-ops. A rename
  // of `COMMANDS`, a move to a macro, or a different bracket shape in
  // `generate_handler!` would do it, and the check would go on passing while
  // the shell it guards was unguarded. So the floor is asserted, not assumed.
  const MINIMUM_COMMANDS = 8;
  if (handler.size < MINIMUM_COMMANDS || manifest.size < MINIMUM_COMMANDS) {
    console.error("desktop native command ACL check failed");
    console.error(
      `Read ${handler.size} commands from generate_handler! and ${manifest.size} from build.rs COMMANDS, ` +
        `which is below the ${MINIMUM_COMMANDS} this check needs to be checking anything. ` +
        "The parsers below have almost certainly stopped matching the source rather than the shell having shrunk.",
    );
    process.exit(1);
  }

  const problems = [];
  for (const command of handler) {
    if (!manifest.has(command)) {
      problems.push(
        `${command} is in generate_handler! but not in build.rs COMMANDS, so no ACL permission exists for it`,
      );
    }
    const permission = `allow-${command.replaceAll("_", "-")}`;
    if (!granted.has(permission)) {
      problems.push(`${command} is not granted by capabilities/default.json (expected "${permission}")`);
    }
  }
  for (const command of manifest) {
    if (!handler.has(command)) {
      problems.push(`build.rs COMMANDS lists ${command}, which no longer exists in generate_handler!`);
    }
  }

  if (problems.length > 0) {
    console.error("desktop native command ACL check failed");
    console.error("");
    for (const problem of problems) {
      console.error(`- ${problem}`);
    }
    console.error("");
    console.error("A command must appear in src/main.rs generate_handler!, in build.rs COMMANDS,");
    console.error("and as allow-<kebab-case-name> in capabilities/default.json.");
    process.exit(1);
  }

  console.log(`${handler.size} native commands are declared, permitted and granted`);
}

function commandsInGenerateHandler(source) {
  const start = source.indexOf("generate_handler![");
  if (start < 0) {
    console.error("desktop native command ACL check failed");
    console.error("Could not find tauri::generate_handler![…] in src-tauri/src/main.rs.");
    process.exit(1);
  }
  const open = source.indexOf("[", start);
  const end = source.indexOf("]", open);
  if (end < 0) {
    console.error("desktop native command ACL check failed");
    console.error("tauri::generate_handler![…] in src-tauri/src/main.rs is not closed.");
    process.exit(1);
  }
  return identifiers(source.slice(open + 1, end));
}

function commandsInBuildScript(source) {
  // The type is `&[&str]`, so the first bracket after `COMMANDS` belongs to the
  // type and not to the list: anchor on the initialiser instead.
  const list = /const\s+COMMANDS\s*:[^=]*=\s*&\[([\s\S]*?)\]\s*;/.exec(source);
  if (!list) {
    console.error("desktop native command ACL check failed");
    console.error("Could not read the COMMANDS list from src-tauri/build.rs.");
    process.exit(1);
  }
  return new Set([...list[1].matchAll(/"([a-z0-9_]+)"/g)].map((match) => match[1]));
}

function identifiers(block) {
  return new Set(
    block
      .split("\n")
      .map((line) => line.replace(/\/\/.*$/, "").trim().replace(/,$/, "").trim())
      .filter((line) => /^[a-z0-9_]+$/.test(line)),
  );
}
