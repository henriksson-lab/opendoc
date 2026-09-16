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
// A CDP command that gets no response is a harness failure, not a reason to
// leave Chrome and the static server running until an outer CI timeout.  The
// limit is deliberately generous for the large WASM cold-load and can be
// raised by a constrained runner without weakening any assertion.
const COMMAND_TIMEOUT_MS = Number(process.env.E2E_CDP_COMMAND_TIMEOUT_MS ?? 30_000);
const CHILD_EXIT_TIMEOUT_MS = Number(process.env.E2E_CHROME_EXIT_TIMEOUT_MS ?? 3_000);
const PROFILE_REMOVE_TIMEOUT_MS = Number(process.env.E2E_PROFILE_REMOVE_TIMEOUT_MS ?? 10_000);

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Chrome's browser process can exit just before a utility process finishes a
 * final profile write.  `rmSync(..., { force: true })` is not a promise that
 * the directory is already quiescent: on Linux it can still report ENOTEMPTY.
 * Wait for a successful removal, which is the useful evidence that every
 * profile writer has stopped, rather than turning a harmless shutdown race
 * into either a leaked profile or a false-green cleanup.
 */
async function removeProfileAfterChromeQuits(profile) {
  const deadline = Date.now() + PROFILE_REMOVE_TIMEOUT_MS;
  let lastError;
  while (true) {
    try {
      rmSync(profile, { recursive: true, force: true, maxRetries: 0 });
      return;
    } catch (error) {
      lastError = error;
      if (Date.now() >= deadline) {
        throw new Error(
          `could not remove Chrome profile ${profile} after ${PROFILE_REMOVE_TIMEOUT_MS}ms: ${String(lastError?.message ?? lastError)}`,
        );
      }
      await sleep(50);
    }
  }
}

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
  let closing = false;

  function rejectPending(error) {
    for (const [id, entry] of pending) {
      pending.delete(id);
      clearTimeout(entry.timer);
      entry.reject(error);
    }
  }

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
      // Do not leave a native dialog holding the page while a navigation or
      // an evaluation waits. Use the normal bounded request path rather than
      // installing an unbounded dummy entry in `pending`.
      void send(
        "Page.handleJavaScriptDialog",
        { accept: true },
        message.sessionId,
      ).catch(() => {
        // The target may already be closing. Its close handler rejects the
        // page command that was waiting, which is the useful test failure.
      });
      return;
    }
    const entry = pending.get(message.id);
    if (!entry) return;
    pending.delete(message.id);
    clearTimeout(entry.timer);
    if (message.error) entry.reject(new Error(message.error.message));
    else entry.resolve(message.result);
  });
  socket.addEventListener("close", () => {
    rejectPending(new Error(closing ? "CDP socket closed during teardown" : "CDP socket closed unexpectedly"));
  });
  socket.addEventListener("error", () => {
    rejectPending(new Error("CDP socket failed"));
  });
  child.once("error", (error) => rejectPending(new Error(`Chrome process failed: ${error.message}`)));
  child.once("exit", (code, signal) => {
    if (!closing) rejectPending(new Error(`Chrome exited unexpectedly (code ${code ?? "null"}, signal ${signal ?? "none"})`));
  });

  function send(method, params = {}, sessionId) {
    const id = (nextId += 1);
    const payload = { id, method, params };
    if (sessionId) payload.sessionId = sessionId;
    if (socket.readyState !== 1) {
      return Promise.reject(new Error(`cannot send CDP ${method}: socket is not open`));
    }
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (!pending.delete(id)) return;
        reject(new Error(`CDP ${method} (id ${id}) timed out after ${COMMAND_TIMEOUT_MS}ms`));
      }, COMMAND_TIMEOUT_MS);
      pending.set(id, { resolve, reject, timer });
      try {
        socket.send(JSON.stringify(payload));
      } catch (error) {
        pending.delete(id);
        clearTimeout(timer);
        reject(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  async function attachPage() {
    const { targetId } = await send("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
    sessions.set(targetId, sessionId);
    await send("Page.enable", {}, sessionId);
    await send("Runtime.enable", {}, sessionId);
    return { targetId, sessionId };
  }

  const { sessionId } = await attachPage();

  function page(sessionId, close) {
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
      let result;
      try {
        result = await send(
          "Runtime.evaluate",
          { expression, awaitPromise: true, returnByValue: true },
          sessionId,
        );
      } catch (error) {
        // A bare `Runtime.evaluate (id N)` cannot identify which assertion
        // stalled once a suite has made hundreds of page calls. Keep the
        // source small (and free of runtime values such as credentials), but
        // name the evaluation so a timeout points to its real owner.
        const label = fn
          .toString()
          .replace(/\s+/g, " ")
          .slice(0, 180);
        throw new Error(`${String(error?.message ?? error)} while evaluating ${label}`);
      }
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
        Delete: { windowsVirtualKeyCode: 46 },
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
    /**
     * A second tab in *this* browser — same profile, so same origin storage
     * and the same Web Lock namespace.
     *
     * `launchChrome` twice gives two profiles, which is right for two
     * collaborators and wrong for anything testing storage shared between
     * tabs: separate profiles share no IndexedDB and contend for no lock.
     */
    async newPage() {
      const { targetId, sessionId: tabSession } = await attachPage();
      return page(tabSession, async () => {
        await send("Target.closeTarget", { targetId });
        sessions.delete(targetId);
      });
    },
    close,
    };
  }

  return page(sessionId, async () => {
    closing = true;
    rejectPending(new Error("CDP session closed during teardown"));
    try {
      socket.close();
    } catch {
      /* already closed */
    }
    if (child.exitCode === null && !child.killed) {
      const exited = new Promise((resolve) => child.once("exit", resolve));
      child.kill("SIGTERM");
      await Promise.race([exited, new Promise((resolve) => setTimeout(resolve, CHILD_EXIT_TIMEOUT_MS))]);
      if (child.exitCode === null) {
        child.kill("SIGKILL");
        await Promise.race([exited, new Promise((resolve) => setTimeout(resolve, CHILD_EXIT_TIMEOUT_MS))]);
      }
    }
    await removeProfileAfterChromeQuits(profile);
  });
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
      // `server.close()` waits for keep-alive sockets. Chrome's test shutdown
      // deliberately does not wait forever for its child process, so leaving
      // those sockets open here made a completed E2E run hang in teardown.
      // They belong solely to this throwaway static artifact server.
      server.closeAllConnections?.();
      await new Promise((done) => server.close(done));
    },
  };
}
