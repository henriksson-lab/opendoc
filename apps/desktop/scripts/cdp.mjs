// Minimal Chrome DevTools Protocol client.
//
// No npm dependency: Chrome is driven over its own WebSocket protocol, and the
// WebSocket client is Node's own (Node 20 exposes it behind
// --experimental-websocket; Node 22+ has it unflagged). Callers must run node
// with that flag — `npm run e2e` does.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";

const CHROME_CANDIDATES = [
  "/usr/bin/google-chrome",
  "/usr/bin/google-chrome-stable",
  "/usr/bin/chromium",
  "/usr/bin/chromium-browser",
  "/snap/bin/chromium",
];

// Snap-packaged Chromium refuses to expose its devtools port under this
// harness, so a "Chrome for Testing" download (what puppeteer installs) is
// preferred when present: unconfined, current, and built for automation.
// Discovered by glob so a version bump does not need a code change.
function chromeForTestingCandidates() {
  const roots = [
    join(homedir(), ".cache", "puppeteer", "chrome"),
    join(homedir(), ".cache", "puppeteer", "chrome-headless-shell"),
  ];
  const found = [];
  for (const root of roots) {
    if (!existsSync(root)) continue;
    for (const release of readdirSync(root)) {
      for (const [dir, binary] of [
        ["chrome-linux64", "chrome"],
        ["chrome-headless-shell-linux64", "chrome-headless-shell"],
      ]) {
        const path = join(root, release, dir, binary);
        if (existsSync(path)) found.push(path);
      }
    }
  }
  return found;
}

// Below this, the browser is too old to be evidence. MathML Core (used to
// render equations) landed in Chrome 109; a machine can easily have an
// ancient /usr/bin/google-chrome alongside a current chromium, and testing
// against the old one silently produces wrong answers rather than errors.
const MINIMUM_MAJOR = 109;

function describeBinary(binary) {
  const probe = spawnSync(binary, ["--version"], { encoding: "utf8", timeout: 20000 });
  const banner = probe.stdout ?? "";
  const match = /(\d+)\.\d+\.\d+/.exec(banner);
  // A snap-confined build reports itself as "… snap". Those refuse to expose a
  // devtools port under this harness (they hang rather than erroring), so they
  // rank below any unconfined build regardless of being newer.
  const confined = /snap/i.test(banner) || binary.startsWith("/snap/");
  return { path: binary, major: match ? Number(match[1]) : 0, confined };
}

/** Picks the newest usable browser rather than the first one found. */
export function findChrome() {
  if (process.env.CHROME_PATH) {
    if (!existsSync(process.env.CHROME_PATH)) {
      throw new Error(`CHROME_PATH does not exist: ${process.env.CHROME_PATH}`);
    }
    return process.env.CHROME_PATH;
  }
  const available = [...chromeForTestingCandidates(), ...CHROME_CANDIDATES]
    .filter((path) => existsSync(path))
    .map(describeBinary);
  if (available.length === 0) {
    throw new Error(
      `no Chrome binary found. Tried: ${CHROME_CANDIDATES.join(", ")}. Set CHROME_PATH to override.`,
    );
  }
  available.sort((a, b) => Number(a.confined) - Number(b.confined) || b.major - a.major);
  const best = available[0];
  if (best.major < MINIMUM_MAJOR) {
    throw new Error(
      `newest browser found is ${best.path} (major ${best.major}), older than the minimum ${MINIMUM_MAJOR}. ` +
        `Results from it would not be trustworthy — install a current Chromium or set CHROME_PATH.`,
    );
  }
  return best.path;
}

async function waitFor(label, attempt, { tries = 60, delayMs = 250 } = {}) {
  let lastError;
  for (let i = 0; i < tries; i += 1) {
    try {
      const value = await attempt();
      if (value) return value;
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, delayMs));
  }
  throw new Error(`timed out waiting for ${label}${lastError ? `: ${lastError.message}` : ""}`);
}

/** Launches headless Chrome and returns a connected CDP session. */
export async function launchChrome({ port = 9333 } = {}) {
  if (typeof WebSocket !== "function") {
    throw new Error("global WebSocket missing: run node with --experimental-websocket");
  }
  const binary = findChrome();
  // Snap-confined Chromium cannot read a profile under /tmp, and simply hangs
  // instead of reporting why. $HOME/.cache is inside the snap's permitted
  // paths, so put the throwaway profile there and fall back to /tmp only if
  // that is somehow unavailable.
  const profileRoot = join(homedir(), ".cache", "opendoc-e2e");
  let profile;
  try {
    mkdirSync(profileRoot, { recursive: true });
    profile = mkdtempSync(join(profileRoot, "run-"));
  } catch {
    profile = mkdtempSync(join(tmpdir(), "opendoc-e2e-"));
  }
  const child = spawn(
    binary,
    [
      "--headless=new",
      "--no-sandbox",
      "--disable-gpu",
      "--disable-dev-shm-usage",
      "--hide-scrollbars",
      "--window-size=1280,900",
      `--user-data-dir=${profile}`,
      `--remote-debugging-port=${port}`,
      "about:blank",
    ],
    { stdio: "ignore" },
  );

  const version = await waitFor("chrome devtools endpoint", async () => {
    const response = await fetch(`http://127.0.0.1:${port}/json/version`);
    return response.ok ? response.json() : null;
  });

  const socket = new WebSocket(version.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", () => reject(new Error("CDP socket failed")), { once: true });
  });

  let nextId = 0;
  const pending = new Map();
  const sessions = new Map();
  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    // Accept native dialogs rather than leaving them to block the page.
    // `Page.enable` makes Chrome hand every JavaScript dialog to its debugging
    // client instead of auto-dismissing it, and a beforeunload prompt (the
    // unsaved-changes guard raises one) then stalls Page.navigate forever with
    // no error. A process that actually crashed would never run beforeunload
    // at all, so accepting is also the truthful answer for a crash test.
    if (message.method === "Page.javascriptDialogOpening") {
      const id = (nextId += 1);
      socket.send(
        JSON.stringify({
          id,
          method: "Page.handleJavaScriptDialog",
          params: { accept: true },
          sessionId: message.sessionId,
        }),
      );
      pending.set(id, { resolve() {}, reject() {} });
      return;
    }
    const entry = pending.get(message.id);
    if (!entry) return;
    pending.delete(message.id);
    if (message.error) entry.reject(new Error(message.error.message));
    else entry.resolve(message.result);
  });

  function send(method, params = {}, sessionId) {
    const id = (nextId += 1);
    const payload = { id, method, params };
    if (sessionId) payload.sessionId = sessionId;
    socket.send(JSON.stringify(payload));
    return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
  }

  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  sessions.set(targetId, sessionId);
  await send("Page.enable", {}, sessionId);
  await send("Runtime.enable", {}, sessionId);

  return {
    async goto(url) {
      await send("Page.navigate", { url }, sessionId);
      // Page.navigate resolves as soon as the navigation starts. The app
      // fetches a ~16 MB WebAssembly core on every load, so callers polling
      // for app state immediately would race it on a loaded machine.
      await waitFor(
        `document load for ${url}`,
        async () => {
          const result = await send(
            "Runtime.evaluate",
            { expression: "document.readyState", returnByValue: true },
            sessionId,
          );
          return result.result.value === "complete";
        },
        { tries: 240, delayMs: 250 },
      );
    },
    /** Evaluates `fn` in the page. `fn` must be self-contained (no closure). */
    async evaluate(fn, ...args) {
      const expression = `(${fn.toString()})(${args.map((a) => JSON.stringify(a)).join(",")})`;
      const result = await send(
        "Runtime.evaluate",
        { expression, awaitPromise: true, returnByValue: true },
        sessionId,
      );
      if (result.exceptionDetails) {
        const detail = result.exceptionDetails;
        throw new Error(
          `page threw: ${detail.exception?.description ?? detail.text ?? "unknown"}`,
        );
      }
      return result.result.value;
    },
    /** Polls `fn` in the page until it returns truthy. */
    async waitUntil(label, fn, options) {
      return waitFor(label, () => this.evaluate(fn), options);
    },
    /**
     * Types text through Chrome's real input pipeline, so contenteditable,
     * beforeinput and IME behave as they do for a user. This is the part
     * jsdom cannot emulate.
     */
    async type(text) {
      for (const char of text) {
        // A keyDown carrying `text` already commits the character. Sending a
        // separate `char` event as well inserts it twice — which looks exactly
        // like the double-dispatch bug this suite guards against, so don't.
        await send("Input.dispatchKeyEvent", { type: "keyDown", text: char, key: char }, sessionId);
        await send("Input.dispatchKeyEvent", { type: "keyUp", key: char }, sessionId);
      }
    },
    /** Presses a named key (Enter, Backspace, ArrowLeft, …) with optional modifiers. */
    async press(key, { modifiers = 0 } = {}) {
      const map = {
        Enter: { windowsVirtualKeyCode: 13, text: "\r" },
        Backspace: { windowsVirtualKeyCode: 8 },
        Tab: { windowsVirtualKeyCode: 9 },
        ArrowLeft: { windowsVirtualKeyCode: 37 },
        ArrowRight: { windowsVirtualKeyCode: 39 },
        ArrowUp: { windowsVirtualKeyCode: 38 },
        ArrowDown: { windowsVirtualKeyCode: 40 },
      };
      const base = map[key] ?? {};
      // As in `type`: a keyDown carrying `text` already commits. Adding a
      // separate `char` event makes one Enter split the paragraph twice.
      const common = { key, modifiers, ...base };
      await send("Input.dispatchKeyEvent", { type: "keyDown", ...common }, sessionId);
      await send("Input.dispatchKeyEvent", { type: "keyUp", ...common }, sessionId);
    },
    /** Clicks the centre of the first element matching `selector`. */
    async click(selector) {
      const box = await this.evaluate((sel) => {
        const node = document.querySelector(sel);
        if (!node) return { error: "not found" };
        const rect = node.getBoundingClientRect();
        if (rect.width === 0 && rect.height === 0) return { error: "has no box" };
        const x = rect.left + rect.width / 2;
        const y = rect.top + rect.height / 2;
        // A mouse event is delivered to whatever is topmost at these
        // coordinates. An element can have a box and still be covered (a
        // closed menu, an overlay), in which case a coordinate click silently
        // lands somewhere else and the test reports a bug that does not exist.
        const hit = document.elementFromPoint(x, y);
        if (!hit || !(hit === node || node.contains(hit) || hit.contains(node))) {
          const describe = (el) =>
            el ? `${el.tagName.toLowerCase()}${el.className ? "." + String(el.className).split(" ").join(".") : ""}` : "nothing";
          return { error: `is covered at (${Math.round(x)},${Math.round(y)}) by ${describe(hit)}` };
        }
        return { x, y };
      }, selector);
      if (box?.error) throw new Error(`click target ${selector} ${box.error}`);
      const common = { x: box.x, y: box.y, button: "left", clickCount: 1 };
      await send("Input.dispatchMouseEvent", { type: "mousePressed", ...common }, sessionId);
      await send("Input.dispatchMouseEvent", { type: "mouseReleased", ...common }, sessionId);
    },
    async screenshot(path) {
      const { data } = await send("Page.captureScreenshot", { format: "png" }, sessionId);
      const { writeFileSync } = await import("node:fs");
      writeFileSync(path, Buffer.from(data, "base64"));
      return path;
    },
    async close() {
      try {
        socket.close();
      } catch {
        /* already closed */
      }
      child.kill("SIGTERM");
      await new Promise((resolve) => setTimeout(resolve, 200));
      rmSync(profile, { recursive: true, force: true });
    },
  };
}

const CONTENT_TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".wasm": "application/wasm",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
};

/**
 * Serves the built `dist/` over HTTP.
 *
 * Deliberately not vite: a second vite instance contends with an already
 * running one over the shared `node_modules/.vite` cache and hangs. Serving
 * the build also means these checks run against the artifact that actually
 * ships rather than against on-the-fly transpilation.
 */
export async function startStaticServer({ port, root }) {
  const { createServer } = await import("node:http");
  const { readFile, stat } = await import("node:fs/promises");
  const { extname, normalize, resolve } = await import("node:path");

  const base = resolve(root);
  try {
    await stat(join(base, "index.html"));
  } catch {
    throw new Error(`no build to serve at ${base} — run \`npm run build\` first`);
  }

  const server = createServer(async (request, response) => {
    try {
      const requested = decodeURIComponent((request.url ?? "/").split("?")[0]);
      const relative = normalize(requested).replace(/^(\.\.[/\\])+/, "");
      let filePath = resolve(base, `.${relative.startsWith("/") ? relative : `/${relative}`}`);
      if (!filePath.startsWith(base)) {
        response.writeHead(403).end("forbidden");
        return;
      }
      let info = await stat(filePath).catch(() => null);
      if (info?.isDirectory()) {
        filePath = join(filePath, "index.html");
        info = await stat(filePath).catch(() => null);
      }
      if (!info) {
        response.writeHead(404).end("not found");
        return;
      }
      const body = await readFile(filePath);
      response.writeHead(200, {
        "content-type": CONTENT_TYPES[extname(filePath)] ?? "application/octet-stream",
        "content-length": body.length,
      });
      response.end(body);
    } catch (error) {
      response.writeHead(500).end(String(error));
    }
  });

  await new Promise((resolveListen, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", resolveListen);
  });

  return {
    url: `http://127.0.0.1:${port}/`,
    async close() {
      await new Promise((done) => server.close(done));
    },
  };
}
