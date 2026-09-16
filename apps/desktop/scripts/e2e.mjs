// Real-browser end-to-end checks (PLAN77 G2 / parity QA-3).
//
// Why this exists alongside scripts/smoke.mjs: the jsdom smoke test drives the
// same code but has no layout engine and no real input pipeline, so it passes
// markup that Chrome renders wrong. That happened for real — a `table-layout:
// fixed` grid silently scaled every specified column width down, and jsdom
// could not see it. Every assertion here is deliberately one that jsdom cannot
// make: computed geometry, computed styles, or text produced by Chrome's own
// key handling through contenteditable.
//
// Serves the built dist/ on its own port (never vite: a second vite instance
// fights the running one over node_modules/.vite and hangs), so it cannot
// disturb a dev server in use and it checks the artifact that actually ships.
// The npm entrypoint runs `build:app` first, so this checks a WebAssembly
// artifact made from the same Rust sources as the service it starts.  A plain
// frontend-only build can otherwise copy a stale WASM collaboration protocol.
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { spawn, spawnSync } from "node:child_process";
import { join } from "node:path";
import { launchChrome, startStaticServer } from "./cdp.mjs";

const desktopRoot = new URL("..", import.meta.url).pathname;
const PORT = Number(process.env.E2E_PORT ?? 10185);
const CDP_PORT = Number(process.env.E2E_CDP_PORT ?? 9399);
const artifacts = process.env.E2E_ARTIFACTS ?? desktopRoot;
const CHECK_TIMEOUT_MS = Number(process.env.E2E_CHECK_TIMEOUT_MS ?? 120_000);

const results = [];
let failed = 0;

async function withCheckTimeout(name, fn) {
  let timer;
  try {
    return await Promise.race([
      fn(),
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`check exceeded ${CHECK_TIMEOUT_MS}ms`)), CHECK_TIMEOUT_MS);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

async function check(name, fn) {
  const startedAt = Date.now();
  // Results used to be printed only after every check, so a stalled await
  // looked like it happened "after convergence" with no exact owner. This is
  // progress telemetry, not an assertion: the existing check still decides
  // pass/fail and retains its complete error message in the final summary.
  console.error(`.. e2e: ${name}`);
  try {
    await withCheckTimeout(name, fn);
    results.push(`  ok   ${name} (${Date.now() - startedAt}ms)`);
  } catch (error) {
    failed += 1;
    results.push(`  FAIL ${name} (${Date.now() - startedAt}ms)\n       ${error.message.split("\n")[0]}`);
  }
}

const settle = (chrome) => chrome.evaluate(() => new Promise((r) => setTimeout(r, 120)));

/**
 * Returns to the home screen through the app's own navigation rather than
 * reloading: a reload re-fetches the ~16 MB WebAssembly core each time, and
 * in-app navigation is also the path the listener-stacking bug lived on.
 */
/**
 * Answers the unsaved-changes guard if it appears. Leaving a dirty document
 * now raises a confirm modal (that guard is the point), so navigation in this
 * suite has to answer it rather than clicking through it.
 */
async function dismissModal(chrome) {
  return chrome.evaluate(() => {
    const dialog = document.querySelector("dialog.modal[open]");
    if (!dialog) return false;
    const confirm =
      dialog.querySelector("button.primary, button[type=submit], [data-confirm]") ??
      dialog.querySelector("button");
    confirm?.click();
    return true;
  });
}

/** Answers the unsaved-changes guard if it appeared, and waits for it to clear. */
async function confirmIfGuarded(chrome) {
  for (let i = 0; i < 40; i += 1) {
    await chrome.evaluate(() => new Promise((r) => setTimeout(r, 100)));
    if (!(await dismissModal(chrome))) {
      const clear = await chrome.evaluate(() => !document.querySelector("dialog.modal[open]"));
      if (clear && i > 0) return;
      if (clear && i === 0) return;
    }
  }
}

async function goHome(chrome) {
  const wentHome = await chrome.evaluate(() => {
    const home = document.querySelector('[data-action="go-home"], [data-action="home"]');
    if (!home) return false;
    home.click();
    return true;
  });
  if (!wentHome) return false;
  // The guard modal may appear a beat after the click, and answering it may
  // itself take a beat to tear down. Poll for whichever of the two states
  // settles rather than assuming a fixed delay.
  for (let i = 0; i < 120; i += 1) {
    if (await dismissModal(chrome)) {
      await chrome.evaluate(() => new Promise((r) => setTimeout(r, 100)));
      continue;
    }
    const atHome = await chrome.evaluate(
      () =>
        !document.querySelector("dialog.modal[open]") &&
        !!document.querySelector('[data-action="new-document"]'),
    );
    if (atHome) return true;
    await chrome.evaluate(() => new Promise((r) => setTimeout(r, 250)));
  }
  throw new Error("could not reach the home screen: a modal never cleared");
}

async function openBlankDocument(chrome) {
  if (!(await goHome(chrome))) {
    await chrome.waitUntil(
      "home screen",
      () => !!document.querySelector('[data-action="new-document"]'),
      { tries: 240, delayMs: 250 },
    );
  }
  await chrome.click('[data-action="new-document"]');
  // Going home does not close the document, so creating a new one over unsaved
  // work is refused until the guard is answered. Answering it is the point of
  // the guard; this suite accepts the loss and continues.
  await confirmIfGuarded(chrome);
  await chrome.waitUntil(
    "editor host",
    () => !!document.querySelector('[contenteditable="true"]'),
    { tries: 240, delayMs: 250 },
  );
  // The caret must sit inside a run span, not on the host: the host's trailing
  // caret-anchor <br> is not a position the editor can resolve to a document
  // offset, so typing there is silently dropped.
  await chrome.evaluate(() => {
    const host = document.querySelector('[contenteditable="true"]');
    host.focus();
    const run = host.querySelector("[data-inline-id]") ?? host;
    const range = document.createRange();
    range.selectNodeContents(run);
    range.collapse(false);
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
  });
}

async function openBlankSpreadsheet(chrome) {
  if (!(await goHome(chrome))) {
    await chrome.waitUntil(
      "home screen",
      () => !!document.querySelector('[data-action="new-spreadsheet"]'),
      { tries: 240, delayMs: 250 },
    );
  }
  await chrome.click('[data-action="new-spreadsheet"]');
  await confirmIfGuarded(chrome);
  await chrome.waitUntil("grid", () => !!document.querySelector("[data-grid] table"), {
    tries: 240,
    delayMs: 250,
  });
}

// ---- Collaboration harness ------------------------------------------------
//
// The collaboration checks need a real `opendoc-service` process and a second
// real browser. A second *browser*, not a second tab: two tabs on one origin
// share one IndexedDB, and only one of them owns it, so the other would be
// memory-only — real behaviour, but not the shape of "two people", and its
// consequences would be mistaken for collaboration bugs. Two Chrome instances
// have two profiles and therefore two independent stores.
//
// The two-tab case is checked on its own at the end of this file.

const SERVICE_PORT = Number(process.env.E2E_SERVICE_PORT ?? PORT + 1);
const ALICE_KEY = "alice-api-key-0123456789";
const BOB_KEY = "bob-api-key-0123456789";
const repositoryRoot = new URL("../../..", import.meta.url).pathname;

/**
 * Starts the collaboration service, building it first if it is not there.
 *
 * `OPENDOC_SERVICE_ORIGINS` is the allowlist the service refuses browsers
 * without: an unconfigured service is reachable by programs and by no page at
 * all, so this is also a check that the allowlist is what makes the page work.
 */
async function startCollaborationService(origin, extraEnv = {}) {
  const binary = join(repositoryRoot, "target", "release", "opendoc-service");
  // Always build. The previous version built only when the binary was *absent*,
  // so a change to `opendoc-service` since the last build was tested against
  // the stale binary and passed — a gate that lies. Cargo is a no-op when the
  // binary is already fresh, so this costs nothing in the common case.
  const built = spawnSync(
    "cargo",
    ["build", "--release", "-p", "opendoc-service", "--bin", "opendoc-service"],
    { cwd: repositoryRoot, stdio: "inherit" },
  );
  if (built.status !== 0) throw new Error("could not build opendoc-service");
  if (!existsSync(binary)) throw new Error(`opendoc-service was not built at ${binary}`);
  const store = mkdtempSync(join(tmpdir(), "opendoc-e2e-service-"));
  const start = () =>
    spawn(binary, [], {
      stdio: "ignore",
      env: {
        ...process.env,
        OPENDOC_SERVICE_ADDR: `127.0.0.1:${SERVICE_PORT}`,
        OPENDOC_SERVICE_ROOT: store,
        OPENDOC_SERVICE_SUBJECTS: `alice:actor-alice:${ALICE_KEY},bob:actor-bob:${BOB_KEY}`,
        OPENDOC_SERVICE_ORIGINS: origin,
        ...extraEnv,
      },
    });
  let child = start();
  const waitForHealth = async () => {
    for (let i = 0; i < 200; i += 1) {
      try {
        const response = await fetch(`http://127.0.0.1:${SERVICE_PORT}/v1/health`);
        if (response.ok) return;
      } catch {
        /* not up yet */
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw new Error("opendoc-service did not become healthy");
  };
  await waitForHealth();
  return {
    url: `http://127.0.0.1:${SERVICE_PORT}`,
    async stop() {
      if (!child) return;
      const dying = child;
      child = null;
      dying.kill("SIGTERM");
      for (let i = 0; i < 100; i += 1) {
        try {
          await fetch(`http://127.0.0.1:${SERVICE_PORT}/v1/health`);
        } catch {
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
    },
    async close() {
      await this.stop();
      rmSync(store, { recursive: true, force: true });
    },
  };
}

/** A session token, fetched from Node — which sends no `Origin`, so the
 *  allowlist does not apply to it. This is the operator path, not the page's. */
async function serviceToken(service, subject, apiKey) {
  const response = await fetch(`${service.url}/v1/sessions`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ subject, api_key: apiKey }),
  });
  if (!response.ok) throw new Error(`session for ${subject}: HTTP ${response.status}`);
  return (await response.json()).token;
}

async function grantRole(service, documentUuid, ownerToken, subject, role) {
  const response = await fetch(`${service.url}/v1/documents/${documentUuid}/grants`, {
    method: "PUT",
    headers: { "content-type": "application/json", authorization: `Bearer ${ownerToken}` },
    body: JSON.stringify({ subject, role }),
  });
  if (!response.ok) throw new Error(`granting ${subject} ${role}: HTTP ${response.status}`);
}

/** Connects one browser to the service through the page's own code path. */
async function joinSession(browser, options) {
  return browser.evaluate(async (opts) => {
    const api = window.__OPENDOC_COLLAB__;
    if (!api) return { error: "the page has no collaboration surface" };
    await api.connect(opts);
    return api.status();
  }, options);
}

/**
 * Starts the same connect path without holding CDP's evaluation open while
 * the app asks the user whether to discard local work.
 *
 * Leaving a collaboration session deliberately retains its replica as an
 * ordinary local document. Its own already-service-durable operations are
 * therefore still unsaved to the local repository, and joining again is a
 * document replacement which must ask before discarding it. Awaiting
 * `api.connect()` here made the test command wait for that answer, while the
 * command which could click the dialog could not be issued until it returned:
 * a harness deadlock, not a WebSocket failure. Start the real async action,
 * prove the guard is present, then answer it through Chrome's input path.
 */
async function rejoinSessionAfterLeaving(browser, options) {
  const started = await browser.evaluate((opts) => {
    const api = window.__OPENDOC_COLLAB__;
    if (!api) return { error: "the page has no collaboration surface" };
    void api.connect(opts);
    return { started: true };
  }, options);
  if (started?.error) return started;
  await pollUntil(
    browser,
    "the reconnect discard guard",
    () => {
      const dialog = document.querySelector("dialog.modal[open]");
      return dialog?.textContent?.includes("Join and discard unsaved changes?") &&
        dialog.querySelector("button.primary")
        ? true
        : null;
    },
    null,
    { tries: 80, delayMs: 50 },
  );
  await browser.click("dialog.modal[open] button.primary");
  return browser.evaluate(() => window.__OPENDOC_COLLAB__?.status() ?? { error: "the page has no collaboration surface" });
}

async function collabPhase(browser) {
  return browser.evaluate(() => window.__OPENDOC_COLLAB__?.status().phase ?? "absent");
}

/**
 * Polls `fn` in the page with one argument until it returns something truthy.
 *
 * `chrome.waitUntil` takes no arguments for the page function, and these
 * checks poll for a value that is only known at runtime (a document uuid, a
 * phase, a typed string).
 */
async function pollUntil(browser, label, fn, argument, { tries = 400, delayMs = 150 } = {}) {
  let last;
  for (let i = 0; i < tries; i += 1) {
    try {
      const value = await browser.evaluate(fn, argument);
      if (value) return value;
      last = value;
    } catch (error) {
      last = error.message;
    }
    await new Promise((resolve) => setTimeout(resolve, delayMs));
  }
  throw new Error(`timed out waiting for ${label} (last: ${JSON.stringify(last)})`);
}

async function waitForPhase(browser, phase, label) {
  try {
    return await pollUntil(
      browser,
      label,
      (wanted) => (window.__OPENDOC_COLLAB__?.status().phase === wanted ? wanted : null),
      phase,
    );
  } catch (error) {
    const notice = await browser.evaluate(() => window.__OPENDOC_COLLAB__?.status().notice ?? null);
    throw new Error(`${error.message} (notice: ${JSON.stringify(notice)})`);
  }
}

/** Every character of the document body, as Chrome drew it. */
async function bodyText(browser) {
  return browser.evaluate(() => document.querySelector('[contenteditable="true"]')?.innerText ?? "");
}

async function waitForBodyText(browser, needle, label) {
  return pollUntil(
    browser,
    label,
    (want) => {
      const host = document.querySelector('[contenteditable="true"]');
      return host && host.innerText.includes(want) ? host.innerText : null;
    },
    needle,
  );
}

/** Types into the document through Chrome's real input pipeline. */
async function typeIntoDocument(browser, text) {
  await browser.evaluate(() => {
    const host = document.querySelector('[contenteditable="true"]');
    host.focus();
    const run = host.querySelector("[data-inline-id]") ?? host;
    const range = document.createRange();
    range.selectNodeContents(run);
    range.collapse(false);
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
  });
  await browser.type(text);
}

let server = null;
let chrome = null;
// The second person and the service, created by the collaboration checks and
// torn down in the same `finally` as everything else.
let bobChrome = null;
let collabService = null;

try {
  server = await startStaticServer({ port: PORT, root: join(desktopRoot, "dist") });
  chrome = await launchChrome({ port: CDP_PORT });
  await chrome.goto(server.url);

  // ---- Typing goes through Chrome's real input pipeline -------------------
  // Regression guard: a leaked listener on the editor host made every gesture
  // fire twice, so one keystroke produced two characters and one Enter
  // produced two paragraphs.
  await check("one keystroke types one character", async () => {
    await openBlankDocument(chrome);
    await chrome.type("abc");
    await settle(chrome);
    const text = await chrome.evaluate(
      () => document.querySelector('[contenteditable="true"]').innerText.trim(),
    );
    assert.equal(text, "abc", `editor text was ${JSON.stringify(text)}`);
  });

  // The window title is the only place the unsaved state shows outside the
  // app's own chrome, and only `renderAll` used to write it: typing went
  // through `editorHooks.onResult`, which updated the status line and left
  // the titlebar saying "Untitled document - OpenDoc" on a document with
  // unsaved work in it.
  await check("typing marks the window title unsaved", async () => {
    const title = await chrome.evaluate(() => document.title);
    assert.ok(
      title.includes("\u2022"),
      `window title after typing was ${JSON.stringify(title)}`,
    );
  });

  await check("one Enter creates exactly one new block", async () => {
    const before = await chrome.evaluate(
      () => document.querySelectorAll("[data-block-id]").length,
    );
    await chrome.press("Enter");
    await settle(chrome);
    const after = await chrome.evaluate(
      () => document.querySelectorAll("[data-block-id]").length,
    );
    assert.equal(after, before + 1, `block count went ${before} -> ${after}`);
  });

  // Regression guard: the delegated [data-action] listener was re-attached on
  // every shell rebuild, so after a home round trip one click ran N times.
  await check("actions still fire once after a home round trip", async () => {
    await openBlankDocument(chrome);
    await goHome(chrome);
    await settle(chrome);
    await openBlankDocument(chrome);
    await chrome.type("z");
    await settle(chrome);
    const text = await chrome.evaluate(
      () => document.querySelector('[contenteditable="true"]').innerText.trim(),
    );
    assert.equal(text, "z", `after round trip the editor held ${JSON.stringify(text)}`);
  });

  // ---- Computed styles: jsdom cannot do this -----------------------------
  await check("bold actually renders bold", async () => {
    await openBlankDocument(chrome);
    await chrome.type("bold me");
    await settle(chrome);
    await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const run = host.querySelector("[data-inline-id]") ?? host;
      const range = document.createRange();
      range.selectNodeContents(run);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    });
    // Target the toolbar button specifically: an identical [data-action] also
    // exists as a Format-menu item, and the menu item comes first in the DOM.
    await chrome.click('.tb[data-action="mark:bold"]');
    await settle(chrome);
    const weight = await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const marked = host.querySelector("strong, b, .mark-bold, [data-mark~='bold']");
      if (!marked) return null;
      return window.getComputedStyle(marked).fontWeight;
    });
    assert.ok(weight, "no bold element rendered in the document");
    assert.ok(
      Number(weight) >= 600 || weight === "bold" || weight === "bolder",
      `computed font-weight was ${weight}`,
    );
  });

  // Regression guard, and one only a browser could have found: applying a
  // mark over a selection that crosses a block holding no text used to stop
  // at that block. Pass 1 of `apply_editor_mark` recorded one boundary pair
  // per *text* block and pass 2 read them back positionally over every block
  // in the range, so with a page break after the first paragraph the second
  // got the first's boundaries and the third got none at all — Bold over all
  // three bolded two of them, silently.
  await check("bold reaches every paragraph across a page break", async () => {
    await openBlankDocument(chrome);
    await chrome.type("alpha");
    await chrome.press("Enter");
    await chrome.type("beta");
    await chrome.press("Enter");
    await chrome.type("gamma");
    await settle(chrome);
    // Put the caret back in the first paragraph so the break lands between it
    // and the second, which is what puts a non-text block inside the
    // selection Bold is then applied over.
    await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const run = host.querySelector(":scope > [data-block-id] [data-inline-id]");
      const range = document.createRange();
      range.selectNodeContents(run);
      range.collapse(false);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.dispatchEvent(new Event("selectionchange"));
    });
    await settle(chrome);
    await chrome.evaluate(() => document.querySelector('[data-action="insert-page-break"]').click());
    await settle(chrome);
    const laidOut = await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      return Array.from(host.querySelectorAll(":scope > [data-block-id]")).map((el) => el.getAttribute("data-kind"));
    });
    assert.ok(
      laidOut.includes("page-break"),
      `the page break did not land: ${JSON.stringify(laidOut)}`,
    );
    await chrome.evaluate(() => document.querySelector('[data-action="select-all"]').click());
    await settle(chrome);
    await chrome.click('.tb[data-action="mark:bold"]');
    await settle(chrome);
    const weights = await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      return Array.from(host.querySelectorAll(":scope > [data-block-id]"))
        .filter((el) => el.getAttribute("data-kind") !== "page-break")
        .map((el) => {
          const text = el.textContent.trim();
          const run = el.querySelector("[data-inline-id]") ?? el;
          return { text, weight: Number(window.getComputedStyle(run).fontWeight), runs: el.querySelectorAll("[data-inline-id]").length };
        });
    });
    assert.equal(weights.length, 3, `expected three text blocks, got ${JSON.stringify(weights)}`);
    for (const block of weights) {
      assert.ok(
        block.weight >= 600,
        `"${block.text}" computed font-weight ${block.weight} after Bold over the whole document`,
      );
      // Pass 1 split runs at boundaries pass 2 then never reached, so a block
      // the mark missed was also left carved up for nothing.
      assert.equal(block.runs, 1, `"${block.text}" was split into ${block.runs} runs`);
    }
  });

  // ---- Block properties reach real CSS (PLAN77 B3/B4/B9) ------------------
  // Everything here is a computed value Chrome produced from the renderer's
  // own markup: jsdom would report the declaration back unchanged whether or
  // not a browser could use it.

  /** The first block inside the editor, which is where the caret starts. */
  const firstBlockStyle = (chrome, properties) =>
    chrome.evaluate((wanted) => {
      const block = document.querySelector('[contenteditable="true"] [data-block-id]');
      if (!block) return null;
      const computed = window.getComputedStyle(block);
      const out = {};
      for (const name of wanted) out[name] = computed[name];
      return out;
    }, properties);

  await check("centring a paragraph reaches computed text-align", async () => {
    await openBlankDocument(chrome);
    await chrome.type("centre me");
    await settle(chrome);
    const before = await firstBlockStyle(chrome, ["textAlign"]);
    assert.notEqual(before.textAlign, "center", "the paragraph starts uncentred");
    await chrome.click('.tb[data-action="align:center"]');
    const after = await chrome.waitUntil(
      "centred paragraph",
      () => {
        const block = document.querySelector('[contenteditable="true"] [data-block-id]');
        const align = block && window.getComputedStyle(block).textAlign;
        return align === "center" ? align : null;
      },
      { tries: 40, delayMs: 100 },
    );
    assert.equal(after, "center");
  });

  await check("indenting a paragraph moves its computed margin by half an inch", async () => {
    await openBlankDocument(chrome);
    await chrome.type("indent me");
    await settle(chrome);
    const before = await firstBlockStyle(chrome, ["marginLeft"]);
    await chrome.click('.tb[data-action="indent"]');
    const after = await chrome.waitUntil(
      "indented paragraph",
      () => {
        const block = document.querySelector('[contenteditable="true"] [data-block-id]');
        const margin = block && window.getComputedStyle(block).marginLeft;
        return margin && margin !== "0px" ? margin : null;
      },
      { tries: 40, delayMs: 100 },
    );
    // One indent step is 720 twips = 36pt, and CSS points are 1/72in against
    // 96 CSS px to the inch, so the browser must land on exactly 48px. This is
    // the assertion that catches a wrong twips -> CSS conversion.
    const moved = Math.round(parseFloat(after) - parseFloat(before.marginLeft));
    assert.equal(moved, 48, `margin-left moved ${before.marginLeft} -> ${after}`);

    // And back: outdenting to zero clears the property rather than pinning 0.
    await chrome.click('.tb[data-action="outdent"]');
    const restored = await chrome.waitUntil(
      "outdented paragraph",
      () => {
        const block = document.querySelector('[contenteditable="true"] [data-block-id]');
        const margin = block && window.getComputedStyle(block).marginLeft;
        return margin === "0px" ? margin : null;
      },
      { tries: 40, delayMs: 100 },
    );
    assert.equal(restored, "0px");
  });

  await check("line spacing reaches computed line-height", async () => {
    await openBlankDocument(chrome);
    await chrome.type("spaced out");
    await settle(chrome);
    await chrome.evaluate(() => {
      const select = document.querySelector('[data-toolbar] select[data-select="line-spacing"]');
      // The option values are indexes into the Rust-generated preset list, and
      // the labels come from Rust too — so the gesture names the option the
      // way a person would, not with a wire form like "multiple:2000" that
      // TypeScript no longer knows how to spell.
      const option = Array.from(select.options).find((item) => item.textContent === "Double");
      if (!option) throw new Error(`no Double option: ${Array.from(select.options, (o) => o.textContent).join(", ")}`);
      select.value = option.value;
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    const ratio = await chrome.waitUntil(
      "double-spaced paragraph",
      () => {
        const block = document.querySelector('[contenteditable="true"] [data-block-id]');
        if (!block) return null;
        const computed = window.getComputedStyle(block);
        const height = parseFloat(computed.lineHeight);
        const size = parseFloat(computed.fontSize);
        if (!height || !size) return null;
        const value = height / size;
        return value > 1.5 ? value : null;
      },
      { tries: 40, delayMs: 100 },
    );
    assert.ok(
      Math.abs(ratio - 2) < 0.02,
      `double spacing computed to ${ratio}x the font size, expected 2`,
    );
  });

  // The per-block DOM path: an update re-parses and re-morphs only the
  // fragments whose markup changed, and a fragment's live element is found by
  // the first `data-block-id` inside it. A list run is the case that can go
  // wrong silently — its `<ol>`/`<ul>` wrapper carries no id of its own, so
  // the key comes from its first `<li>`, and getting that wrong rebuilds the
  // whole run on every keystroke. Content stays right (the caret is restored
  // from the model straight afterwards), so only node *identity* shows it:
  // an expando the renderer never writes survives exactly as long as the
  // element does.
  await check("typing elsewhere does not rebuild a list it did not change", async () => {
    await openBlankDocument(chrome);
    await chrome.type("lead paragraph");
    await settle(chrome);
    await chrome.press("Enter");
    await chrome.type("first item");
    await settle(chrome);
    await chrome.click('.tb[data-action="style:list:bullet"]');
    await settle(chrome);
    await chrome.press("Enter");
    await chrome.type("second item");
    await settle(chrome);
    // Out of the list and back into the paragraph above it, so the keystroke
    // below changes a fragment the list run is not part of.
    await chrome.press("ArrowUp");
    await chrome.press("ArrowUp");
    await settle(chrome);
    const marked = await chrome.evaluate(() => {
      const run = document.querySelector('[contenteditable="true"] ul');
      const items = document.querySelectorAll('[contenteditable="true"] ul li').length;
      if (!run) return null;
      run.__e2eProbe = "kept";
      return items;
    });
    assert.equal(marked, 2, `expected a two-item list, saw ${marked}`);
    await chrome.type("X");
    await settle(chrome);
    const survived = await chrome.evaluate(() => {
      const run = document.querySelector('[contenteditable="true"] ul');
      return {
        probe: run?.__e2eProbe ?? null,
        items: document.querySelectorAll('[contenteditable="true"] ul li').length,
        leadText: document.querySelector('[contenteditable="true"] > p')?.textContent ?? "",
      };
    });
    assert.equal(survived.items, 2, "the list lost or gained items");
    assert.ok(
      survived.leadText.includes("X"),
      `the keystroke did not land in the paragraph: ${JSON.stringify(survived.leadText)}`,
    );
    assert.equal(
      survived.probe,
      "kept",
      "the list run was rebuilt by an update that did not change it",
    );
  });

  await check("a checklist checkbox toggles through the document", async () => {
    await openBlankDocument(chrome);
    await chrome.type("task");
    await settle(chrome);
    await chrome.click('.tb[data-action="style:list:checklist"]');
    const initial = await chrome.waitUntil(
      "checklist checkbox",
      () => {
        const input = document.querySelector('[contenteditable="true"] .doc-checkbox input');
        const item = input && input.closest("li");
        return input ? { checked: input.checked, item: item.getAttribute("data-checked") } : null;
      },
      { tries: 60, delayMs: 100 },
    );
    assert.equal(initial.checked, false, "a new checklist item is not ticked");
    assert.equal(initial.item, "false");

    await chrome.click('[contenteditable="true"] .doc-checkbox');
    const ticked = await chrome.waitUntil(
      "ticked checklist item",
      () => {
        const input = document.querySelector('[contenteditable="true"] .doc-checkbox input');
        const item = input && input.closest("li");
        return item && item.getAttribute("data-checked") === "true"
          ? { checked: input.checked }
          : null;
      },
      { tries: 60, delayMs: 100 },
    );
    // The input's *property*, not its attribute: the checkbox is never clicked
    // directly (pointer-events is off on it), so its checkedness still follows
    // the attribute the re-render writes. A user-clickable checkbox would go
    // "dirty" here and stop tracking the document.
    assert.equal(ticked.checked, true, "the rendered checkbox follows the model");

    await chrome.click('[contenteditable="true"] .doc-checkbox');
    const cleared = await chrome.waitUntil(
      "untickled checklist item",
      () => {
        const input = document.querySelector('[contenteditable="true"] .doc-checkbox input');
        const item = input && input.closest("li");
        return item && item.getAttribute("data-checked") === "false"
          ? { checked: input.checked }
          : null;
      },
      { tries: 60, delayMs: 100 },
    );
    assert.equal(cleared.checked, false, "toggling back unticks the rendered checkbox");
  });

  // FS-19: a new spreadsheet must be empty. It shipped seeded with demo
  // content ("Item"/"Apples"/"Total") because the constructor installed a
  // sample workbook.
  await check("a blank spreadsheet is actually blank", async () => {
    await openBlankSpreadsheet(chrome);
    const text = await chrome.evaluate(() => {
      const cells = [...document.querySelectorAll("[data-grid] tbody td")];
      return cells.map((c) => c.textContent.trim()).filter(Boolean).slice(0, 10);
    });
    assert.deepEqual(text, [], `new spreadsheet contained ${JSON.stringify(text)}`);
  });

  // ---- Computed geometry: the class of bug jsdom shipped ------------------
  await check("grid renders at its default geometry", async () => {
    await openBlankSpreadsheet(chrome);
    const geometry = await chrome.evaluate(() => {
      const cell = document.querySelector("[data-grid] tbody tr td");
      const rect = cell?.getBoundingClientRect();
      return rect ? { width: Math.round(rect.width), height: Math.round(rect.height) } : null;
    });
    assert.ok(geometry, "no grid cell rendered");
    assert.ok(
      geometry.width >= 60 && geometry.width <= 140,
      `default column width computed to ${geometry.width}px, expected ~100`,
    );
    assert.ok(
      geometry.height >= 16 && geometry.height <= 40,
      `default row height computed to ${geometry.height}px, expected ~24`,
    );
  });

  // The UI path for resizing is covered by the jsdom smoke test. What only a
  // browser can answer is whether a width the renderer emits survives Chrome's
  // table layout — `table-layout: fixed` on a stretched table silently scales
  // every specified width down, which is exactly what shipped once.
  // NOTE: there was a check here asserting that explicitly specified column
  // widths are not scaled down by table layout — the bug where a 152px column
  // rendered at 64px. It was removed because it could not be made to fail:
  // re-introducing `.sheet-grid { width: 100% }` on the built artifact left it
  // green, so it did not actually guard that mechanism and only implied it
  // did. The "default geometry" check above IS falsifiable (forcing a 30px
  // header width fails it), so broken grid geometry is still caught.
  // Replacing this properly means driving a real width through
  // `set_spreadsheet_column_width` and measuring the result, which needs a
  // test seam the page does not expose yet. Tracked in PLAN77.

  // ---- Find and replace across a formatting boundary (PLAN77 G4 / ED-25) --
  // The bug this guards: a paragraph is stored as several inline runs as soon
  // as part of it is bolded, and the old TypeScript search walked one run at a
  // time, so a query straddling the boundary found nothing at all. Matching
  // now happens in Rust over the whole block's text.
  await check("find and replace crosses a formatting boundary", async () => {
    await openBlankDocument(chrome);
    await chrome.type("hello world");
    await settle(chrome);
    // Bold the last five characters, which splits the paragraph into a plain
    // "hello " run and a bold "world" run.
    await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const text = host.querySelector("[data-inline-id]").firstChild;
      const range = document.createRange();
      range.setStart(text, 6);
      range.setEnd(text, 11);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    });
    await chrome.click('.tb[data-action="mark:bold"]');
    await settle(chrome);
    const runs = await chrome.evaluate(
      () => document.querySelectorAll('[contenteditable="true"] [data-inline-id]').length,
    );
    assert.equal(runs, 2, `expected the paragraph to be split into two runs, found ${runs}`);

    await chrome.evaluate(() => document.querySelector('[data-action="find"]').click());
    await chrome.waitUntil("find bar", () => !!document.querySelector("[data-find-input]"), {
      tries: 60,
      delayMs: 100,
    });
    // "lo wo" starts in the plain run and ends in the bold one.
    await chrome.evaluate(() => {
      const input = document.querySelector("[data-find-input]");
      input.value = "lo wo";
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await chrome.waitUntil(
      "match counter",
      () => {
        const text = document.querySelector(".find-count")?.textContent?.trim();
        return text ? text : null;
      },
      { tries: 60, delayMs: 100 },
    );
    await settle(chrome);
    const count = await chrome.evaluate(
      () => document.querySelector(".find-count")?.textContent?.trim(),
    );
    assert.equal(count, "1 of 1", `a match spanning two runs was counted as ${JSON.stringify(count)}`);

    await chrome.evaluate(() => {
      document.querySelector("[data-replace-input]").value = "LO-WO";
    });
    await chrome.click('[data-action="replace-all"]');
    const replaced = await chrome.waitUntil(
      "replaced text",
      () => {
        const body = document.querySelector('[contenteditable="true"]')?.innerText.trim();
        return body && body !== "hello world" ? body : null;
      },
      { tries: 60, delayMs: 100 },
    );
    assert.equal(
      replaced,
      "helLO-WOrld",
      `after replacing across the boundary the document held ${JSON.stringify(replaced)}`,
    );
  });

  // ---- The nested-list marker cycle (PLAN88 item 9) ----------------------
  // The cycle used to be written out by hand in `styles.css` as well as by
  // `opendoc_layout::list_style_type_rules`, which is what the painted PDF
  // picks its markers from. Two statements of where a finite CSS cycle stops
  // are two chances for the screen and the paper to stop in different places.
  // The stylesheet copy is gone and the rules now arrive with the layout, so
  // this is the check that they really reach the browser — a Rust test can
  // see the string, only a browser can see that it applies.
  //
  // Falsifiable two ways: drop the `applyStyleRule("doc-list-style", ...)`
  // call and the nested list falls back to `disc`; drop the extra class on
  // `.doc-body ul.doc-list.doc-checklist` and the checklist takes `circle`
  // from the later-injected depth rule and draws a bullet beside its
  // checkbox.
  await check("nested list markers come from the layout engine's cycle", async () => {
    await openBlankDocument(chrome);
    await chrome.type("top level");
    await settle(chrome);
    await menuAction("Format", "style:list:bullet");
    await settle(chrome);
    await chrome.press("Enter");
    await chrome.type("nested");
    await settle(chrome);
    await chrome.click('.tb[data-action="indent"]');
    await settle(chrome);

    const markers = await poll("the nested list", () =>
      chrome.evaluate(() => {
        const host = document.querySelector('[contenteditable="true"]');
        const outer = host?.querySelector("ul.depth-0");
        const inner = host?.querySelector("ul.depth-1");
        if (!outer || !inner) return null;
        return {
          outer: window.getComputedStyle(outer).listStyleType,
          inner: window.getComputedStyle(inner).listStyleType,
        };
      }),
    );
    // depth 0 keeps the browser's initial `disc`; depth 1 is the first turn of
    // the cycle. These are the values the deleted stylesheet block stated.
    assert.deepEqual(
      markers,
      { outer: "disc", inner: "circle" },
      `the nested bullet markers were ${JSON.stringify(markers)}`,
    );

    // A checklist draws its checkbox and no bullet, at any depth — the rule
    // that says so has to outrank the injected depth rules.
    await menuAction("Format", "style:list:checklist");
    await settle(chrome);
    const checklist = await poll("the checklist", () =>
      chrome.evaluate(() => {
        const list = document
          .querySelector('[contenteditable="true"]')
          ?.querySelector("ul.doc-checklist");
        return list ? window.getComputedStyle(list).listStyleType : null;
      }),
    );
    assert.equal(
      checklist,
      "none",
      `a checklist drew the marker ${JSON.stringify(checklist)} beside its checkbox`,
    );
  });

  // ---- Stepping onto a match outside the body (PLAN88 item 12) -----------
  // The bug this guards: the Rust search covers the header, the footer and the
  // footnote bodies, but those blocks are not in `document.blocks` and so not
  // in the editor's DOM. The find bar handed every match's positions to
  // `setSelection`, so for a header match that call did nothing whatsoever —
  // the counter advanced to "2 of 2" and the caret stayed in the body, with
  // nothing on screen to say the match was somewhere the editor cannot reach.
  //
  // Falsifiable: drop the `region` branch in `highlightMatch`/`renderFind` and
  // the counter reads a bare "2 of 2" with no Open button, which is the exact
  // silence this replaces.
  await check("a find match in the header says so and opens the header", async () => {
    await openBlankDocument(chrome);
    await chrome.type("needle in the body");
    await settle(chrome);
    await menuAction("Insert", "page-furniture:header");
    await poll("the header prompt", () =>
      answerPrompt("header", { text: "needle in the header", field: "none", alignment: "start" }),
    );
    await settle(chrome);

    await chrome.evaluate(() => document.querySelector('[data-action="find"]').click());
    await chrome.waitUntil("find bar", () => !!document.querySelector("[data-find-input]"), {
      tries: 60,
      delayMs: 100,
    });
    await chrome.evaluate(() => {
      const input = document.querySelector("[data-find-input]");
      input.value = "needle";
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const first = await poll("the match counter", () =>
      chrome.evaluate(() => document.querySelector(".find-count")?.textContent?.trim() || null),
    );
    assert.equal(first, "1 of 2", `the body and the header should both match, counter read ${JSON.stringify(first)}`);
    const bodyGoto = await chrome.evaluate(
      () => document.querySelector(".find-goto")?.hidden ?? null,
    );
    assert.equal(bodyGoto, true, "a body match is selectable, so it must not offer an Open button");

    // Next steps onto the header match — the one that used to do nothing.
    await chrome.click('[data-action="find-next"]');
    const second = await poll("the header match counter", () =>
      chrome.evaluate(() => {
        const text = document.querySelector(".find-count")?.textContent?.trim();
        return text && text.startsWith("2 of 2") ? text : null;
      }),
    );
    assert.equal(
      second,
      "2 of 2 — in the header",
      `stepping onto the header match read ${JSON.stringify(second)}`,
    );
    const goto = await chrome.evaluate(() => {
      const button = document.querySelector(".find-goto");
      return button ? { hidden: button.hidden, label: button.textContent.trim() } : null;
    });
    assert.deepEqual(
      goto,
      { hidden: false, label: "Open the header" },
      `the way into the header read ${JSON.stringify(goto)}`,
    );

    // And it is a way in, not a label: clicking it opens the header editor
    // with the text the match is in.
    await chrome.click('[data-action="find-open-region"]');
    const dialog = await poll("the header dialog", () =>
      chrome.evaluate(() => {
        const open = [...document.querySelectorAll("dialog.modal[open]")].find(
          (node) => node.querySelector("h2")?.textContent === "header",
        );
        if (!open) return null;
        return open.querySelector("form").elements.namedItem("text")?.value ?? "";
      }),
    );
    assert.equal(
      dialog,
      "needle in the header",
      `the header editor opened on ${JSON.stringify(dialog)}`,
    );
    await dismissModal(chrome);
  });

  // ---- Durable browser storage (PLAN77 F3, ADR 0008) ---------------------
  // The browser used to hold documents in memory only: closing the tab lost
  // the work and the crash-recovery journal was inert. These checks are the
  // ones jsdom structurally cannot make — it has no IndexedDB and no page
  // load — and each is falsifiable: without the storage adapter the document
  // is simply not there after the reload, and the recovery offer never comes.
  const STORAGE_REPO = "e2e-repo";

  /** Polls an async predicate, returning its first truthy value. */
  async function poll(label, fn, tries = 200, delayMs = 250) {
    let lastError = null;
    for (let attempt = 0; attempt < tries; attempt += 1) {
      try {
        const value = await fn();
        if (value) return value;
      } catch (error) {
        lastError = error;
      }
      await chrome.evaluate((ms) => new Promise((r) => setTimeout(r, ms)), delayMs);
    }
    throw new Error(`timed out waiting for ${label}${lastError ? `: ${lastError.message}` : ""}`);
  }

  /** Fills and submits the prompt whose heading is `title`, if it is open. */
  async function answerPrompt(title, values) {
    return chrome.evaluate(
      (wanted, fields) => {
        const dialog = [...document.querySelectorAll("dialog.modal[open]")].find(
          (node) => node.querySelector("h2")?.textContent === wanted,
        );
        if (!dialog) return false;
        const form = dialog.querySelector("form");
        for (const [name, value] of Object.entries(fields)) {
          const control = form.elements.namedItem(name);
          if (control) control.value = value;
        }
        form.requestSubmit();
        return true;
      },
      title,
      values,
    );
  }

  /** Opens a menu and clicks one of its items, hit-tested like any click. */
  async function menuAction(menu, action) {
    await chrome.evaluate((label) => {
      const group = document.querySelector(`details.menu-group[data-menu="${label}"]`);
      if (group) group.open = true;
    }, menu);
    await settle(chrome);
    await chrome.click(`details.menu-group[data-menu="${menu}"] [data-action="${action}"]`);
    await chrome.evaluate((label) => {
      const group = document.querySelector(`details.menu-group[data-menu="${label}"]`);
      if (group) group.open = false;
    }, menu);
  }

  /** Reads the app's IndexedDB database directly, which is the real proof. */
  async function storedKeys() {
    return chrome.evaluate(
      () =>
        new Promise((resolve) => {
          const opened = indexedDB.open("opendoc");
          opened.onerror = () => resolve([]);
          opened.onsuccess = () => {
            const database = opened.result;
            if (![...database.objectStoreNames].includes("volume")) {
              database.close();
              resolve([]);
              return;
            }
            const request = database
              .transaction("volume", "readonly")
              .objectStore("volume")
              .getAllKeys();
            request.onerror = () => {
              database.close();
              resolve([]);
            };
            request.onsuccess = () => {
              const keys = [...request.result];
              database.close();
              resolve(keys);
            };
          };
        }),
    );
  }

  async function editorText() {
    return chrome.evaluate(
      () => document.querySelector('[contenteditable="true"]')?.innerText ?? "",
    );
  }

  /** Answers any crash-recovery offer left by an earlier check. */
  async function clearRecoveryOffers() {
    for (let attempt = 0; attempt < 4; attempt += 1) {
      if (!(await answerPrompt("Recover unsaved work?", { choice: "discard" }))) return;
      await settle(chrome);
    }
  }

  await check("saving in the browser writes the repository into IndexedDB", async () => {
    await openBlankDocument(chrome);
    await chrome.type("persisted through indexeddb");
    await settle(chrome);
    await menuAction("File", "save");
    await poll("the save prompt", () => answerPrompt("Save", { path: STORAGE_REPO }));
    const keys = await poll("repository keys in IndexedDB", async () => {
      const stored = await storedKeys();
      const repository = stored.filter((key) => key.startsWith(`repositories/${STORAGE_REPO}/`));
      return repository.length > 0 ? repository : null;
    });
    assert.ok(
      keys.some((key) => key.includes("/objects/")),
      `IndexedDB held no content objects, only ${JSON.stringify(keys.slice(0, 8))}`,
    );
    assert.ok(
      keys.some((key) => key.includes("/heads/")),
      "no branch head reached IndexedDB, so the document would not be findable",
    );
  });

  await check("a saved document survives a page reload", async () => {
    // A real navigation: the WebAssembly core is thrown away and rebuilt, so
    // anything that comes back came out of IndexedDB.
    await chrome.goto(server.url);
    await chrome.waitUntil(
      "home screen after the reload",
      () => !!document.querySelector('[data-action="new-document"]'),
      { tries: 240, delayMs: 250 },
    );
    await clearRecoveryOffers();
    // The home screen carries its own "Open folder…" button; the menu bar
    // belongs to the editor shell and is not rendered here.
    await chrome.click('[data-action="open-repository"]');
    await poll("the open-repository prompt", () =>
      answerPrompt("Open repository", { path: STORAGE_REPO }),
    );
    const text = await poll("the reopened document", async () => {
      const value = await editorText();
      return value.includes("persisted through indexeddb") ? value : null;
    });
    assert.ok(
      text.includes("persisted through indexeddb"),
      `the reopened document held ${JSON.stringify(text)}`,
    );
  });

  await check("the recents list survives a page reload", async () => {
    // The other half of durable browser storage, and the one that was still
    // missing: `recent_documents` lived only in `AppState`, so a reload showed
    // an empty "Recent documents" panel however much had been saved. The list
    // is now stored through `RecentDocuments` under its own volume key, and
    // `opendoc-wasm` installs the store in `storage_ready`.
    //
    // Falsifiable in two independent ways: without the store nothing is keyed
    // `recent/documents` in IndexedDB at all, and without the wiring the home
    // screen renders "No recent documents yet." after the reload.
    const keys = await poll("the recents list in IndexedDB", async () => {
      const stored = await storedKeys();
      return stored.includes("recent/documents") ? stored : null;
    });
    assert.ok(
      keys.includes("recent/documents"),
      `no recents list reached IndexedDB, only ${JSON.stringify(keys.slice(0, 8))}`,
    );

    // A real navigation: the WebAssembly core is thrown away, so the list that
    // comes back came out of IndexedDB and not out of this tab's memory.
    await chrome.goto(server.url);
    await chrome.waitUntil(
      "home screen after the reload",
      () => !!document.querySelector('[data-action="new-document"]'),
      { tries: 240, delayMs: 250 },
    );
    await clearRecoveryOffers();
    const listed = await poll("a remembered repository on the home screen", () =>
      chrome.evaluate(() =>
        [...document.querySelectorAll('.recent-list [data-action="open-recent"]')].map((node) => ({
          root: node.dataset.root,
          uuid: node.dataset.uuid,
          title: node.querySelector(".recent-title")?.textContent?.trim() ?? "",
        })),
      ),
    );
    const remembered = listed.find((recent) => recent.root === STORAGE_REPO);
    assert.ok(
      remembered,
      `the home screen forgot which repositories were used: ${JSON.stringify(listed)}`,
    );
    assert.ok(remembered.uuid, "a remembered document with no uuid cannot be reopened");

    // Remembering it is only useful if it still opens, so click it.
    await chrome.click(
      `.recent-list [data-action="open-recent"][data-uuid="${remembered.uuid}"]`,
    );
    const text = await poll("the document reopened from the recents list", async () => {
      const value = await editorText();
      return value.includes("persisted through indexeddb") ? value : null;
    });
    assert.ok(
      text.includes("persisted through indexeddb"),
      `opening the remembered entry gave ${JSON.stringify(text)}`,
    );
  });

  await check("unsaved work survives a crash (ADR 0005 in the browser)", async () => {
    await openBlankDocument(chrome);
    await chrome.type("crash protected work");
    await settle(chrome);
    const segments = await poll("a recovery segment in IndexedDB", async () => {
      const stored = await storedKeys();
      const recovery = stored.filter((key) => key.startsWith("recovery/"));
      return recovery.length > 0 ? recovery : null;
    });
    assert.ok(segments.length >= 1, "the crash-recovery journal wrote nothing");

    // Reloading is as close to `kill -9` as a test can get: the tab's memory
    // is gone and only what reached IndexedDB is left.
    await chrome.goto(server.url);
    await poll("the crash-recovery offer", () =>
      answerPrompt("Recover unsaved work?", { choice: "recover" }),
    );
    const text = await poll("the recovered text", async () => {
      const value = await editorText();
      return value.includes("crash protected work") ? value : null;
    });
    assert.ok(
      text.includes("crash protected work"),
      `the recovered document held ${JSON.stringify(text)}`,
    );
  });

  // ---- Page geometry and pagination (PLAN77 B7/B8, ADR 0014) -------------
  // Everything here is computed layout. jsdom has none: it would report the
  // `pt` declaration back unchanged and could not tell whether Chrome put a
  // second page anywhere.
  //
  // Since ADR 0014 the page breaks are decided in Rust and the browser only
  // places them, so the check that matters is *agreement*: Rust says a block
  // is on page 2, and Chrome must have drawn it inside page 2's content box.
  // A paginator that is self-consistent but disagrees with the screen is the
  // failure this section exists to catch.

  /** The measured page sheet, in CSS pixels as Chrome laid it out. */
  const sheetGeometry = () =>
    chrome.evaluate(() => {
      const sheet = document.querySelector(".page-sheet");
      const flow = document.querySelector("[data-page]");
      if (!sheet || !flow) return null;
      const rect = sheet.getBoundingClientRect();
      const padding = window.getComputedStyle(flow);
      return {
        width: Math.round(rect.width),
        height: Math.round(rect.height),
        paddingTop: Math.round(parseFloat(padding.paddingTop)),
        paddingLeft: Math.round(parseFloat(padding.paddingLeft)),
        sheets: document.querySelectorAll(".page-sheet").length,
      };
    });

  await check("a US Letter page computes to 816 x 1056 with one-inch margins", async () => {
    await openBlankDocument(chrome);
    await chrome.type("letter");
    await settle(chrome);
    const geometry = await poll("a drawn page sheet", async () => await sheetGeometry());
    // 12240 twips is 8.5in is 612pt is 816 CSS px. The stylesheet says 612pt
    // and Chrome does the conversion; if the projection emitted px it would
    // have had to assume a DPI to get here.
    assert.equal(geometry.width, 816, `page width computed to ${geometry.width}px`);
    assert.equal(geometry.height, 1056, `page height computed to ${geometry.height}px`);
    assert.equal(geometry.paddingTop, 96, `top margin computed to ${geometry.paddingTop}px`);
    assert.equal(geometry.paddingLeft, 96, `left margin computed to ${geometry.paddingLeft}px`);
    assert.equal(geometry.sheets, 1, "a short document is one page");
    // And it must come from the document's own projection, not from the
    // stylesheet fallback that happens to carry the same default.
    const declared = await chrome.evaluate(
      () =>
        document.querySelector("[data-page-stack]")?.style.getPropertyValue("--page-width").trim() ??
        null,
    );
    assert.equal(declared, "612pt", `the page stack declared ${JSON.stringify(declared)}`);
  });

  await check("switching to A4 landscape reshapes the page", async () => {
    await menuAction("File", "page-setup");
    await poll("the page setup prompt", () =>
      answerPrompt("Page setup", { size: "a4", orientation: "landscape" }),
    );
    const geometry = await poll("the reshaped page", async () => {
      const measured = await sheetGeometry();
      return measured && measured.width > measured.height ? measured : null;
    });
    // A4 landscape is 16838 x 11906 twips = 841.9pt x 595.3pt = 1123 x 794 px.
    assert.ok(
      Math.abs(geometry.width - 1123) <= 1,
      `A4 landscape width computed to ${geometry.width}px, expected ~1123`,
    );
    assert.ok(
      Math.abs(geometry.height - 794) <= 1,
      `A4 landscape height computed to ${geometry.height}px, expected ~794`,
    );
  });

  await check("a header renders on the page with its page number resolved", async () => {
    await openBlankDocument(chrome);
    await chrome.type("with a header");
    await settle(chrome);
    await menuAction("Insert", "page-furniture:header");
    await poll("the header prompt", () =>
      answerPrompt("header", { text: "Chapter", field: "page-number", alignment: "center" }),
    );
    const header = await poll("the drawn header", () =>
      chrome.evaluate(() => {
        const box = document.querySelector(".page-sheet .page-sheet-header");
        if (!box) return null;
        const rect = box.getBoundingClientRect();
        const sheet = document.querySelector(".page-sheet").getBoundingClientRect();
        return {
          text: box.innerText.replace(/\s+/g, " ").trim(),
          field: box.querySelector('[data-field="page-number"]')?.textContent ?? null,
          align: window.getComputedStyle(box.querySelector("[data-block-id]")).textAlign,
          // Distance from the top of the sheet: the model says half an inch.
          offsetFromSheetTop: Math.round(rect.top - sheet.top),
          insideEditor: !!box.closest('[contenteditable="true"]'),
        };
      }),
    );
    assert.equal(header.text, "Chapter 1", `the header read ${JSON.stringify(header.text)}`);
    assert.equal(header.field, "1", "the page-number field was not resolved");
    assert.equal(header.align, "center", `header alignment computed to ${header.align}`);
    assert.equal(
      header.offsetFromSheetTop,
      48,
      `header sat ${header.offsetFromSheetTop}px from the sheet top, expected 48 (half an inch)`,
    );
    assert.equal(
      header.insideEditor,
      false,
      "the header must be decoration outside the editable flow, not document content",
    );
  });

  await check("content that overflows the page flows onto a second page", async () => {
    await openBlankDocument(chrome);
    // Shrink the page rather than writing pages of text: pagination is a
    // function of the page box, so a short sheet exercises the same mechanism
    // and keeps the check quick.
    await menuAction("File", "page-setup");
    await poll("the page setup prompt", () =>
      answerPrompt("Page setup", {
        size: "custom",
        // 8.5in wide by 3in tall *is* landscape; the model derives
        // orientation from the dimensions, so saying "portrait" here would
        // correctly rotate the sheet to 3in by 8.5in.
        orientation: "landscape",
        width: "8.50",
        height: "3.00",
        top: "0.50",
        bottom: "0.50",
        start: "0.50",
        end: "0.50",
      }),
    );
    await poll("the short page", async () => {
      const measured = await sheetGeometry();
      return measured && measured.height === 288 ? measured : null;
    });
    for (let i = 0; i < 14; i += 1) {
      await chrome.type(`line ${i}`);
      await chrome.press("Enter");
    }
    const paginated = await poll("a second page", () =>
      chrome.evaluate(() => {
        const sheets = [...document.querySelectorAll(".page-sheet")];
        if (sheets.length < 2) return null;
        const blocks = [...document.querySelectorAll('[contenteditable="true"] [data-block-id]')];
        const second = sheets[1].getBoundingClientRect();
        const flow = document.querySelector("[data-page]");
        const padding = parseFloat(window.getComputedStyle(flow).paddingTop);
        // The first block at or below the second sheet's top must start inside
        // that sheet's content box, not in the gutter above it.
        const firstOnSecond = blocks
          .map((block) => block.getBoundingClientRect())
          .find((rect) => rect.top >= second.top - 1);
        return {
          pages: sheets.length,
          gapAbove: Math.round(second.top - sheets[0].getBoundingClientRect().bottom),
          inset: firstOnSecond ? Math.round(firstOnSecond.top - second.top) : null,
          padding: Math.round(padding),
        };
      }),
    );
    assert.ok(paginated.pages >= 2, `14 paragraphs on a 3in page produced ${paginated.pages} page(s)`);
    assert.ok(
      paginated.gapAbove > 0,
      "the second sheet was drawn on top of the first instead of below it",
    );
    assert.ok(
      paginated.inset !== null && Math.abs(paginated.inset - paginated.padding) <= 2,
      `page 2 started ${paginated.inset}px into the sheet, expected its ${paginated.padding}px top margin`,
    );
  });

  await check("an explicit page break starts the next page", async () => {
    await openBlankDocument(chrome);
    await chrome.type("before the break");
    await chrome.press("Enter");
    await chrome.type("after the break");
    await settle(chrome);
    // Put the caret back in the first paragraph: `insert_page_break_after`
    // breaks after the focused block, so the break has to land between the
    // two paragraphs for the second one to be pushed onto page 2.
    await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const first = host.querySelector("[data-block-id] [data-inline-id]");
      const range = document.createRange();
      range.selectNodeContents(first);
      range.collapse(false);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      host.dispatchEvent(new Event("selectionchange", { bubbles: true }));
      document.dispatchEvent(new Event("selectionchange"));
    });
    await settle(chrome);
    // Ctrl+Enter is the app's own page-break gesture; the paginator has to
    // honour the block it inserts, not only overflow.
    await chrome.press("Enter", { modifiers: 2 });
    const broken = await poll("a forced second page", () =>
      chrome.evaluate(() => {
        const sheets = [...document.querySelectorAll(".page-sheet")];
        if (sheets.length < 2) return null;
        const blocks = [...document.querySelectorAll('[contenteditable="true"] [data-block-id]')];
        const last = blocks[blocks.length - 1].getBoundingClientRect();
        const second = sheets[1].getBoundingClientRect();
        return { pages: sheets.length, onSecond: last.top >= second.top - 1 };
      }),
    );
    assert.equal(broken.pages, 2, `a page break produced ${broken.pages} page(s)`);
    assert.equal(broken.onSecond, true, "the block after the break did not land on page 2");
  });

  await check("a paragraph's space-before survives pagination", async () => {
    // Two owners, one CSS property, and the document lost. `opendoc-render`
    // projected the model's space-before as `margin-top`; pagination writes
    // the page-break margin to the same property and clears it on every block
    // that does not open a page — so applying a layout deleted the spacing the
    // render had just put on the page, permanently and invisibly. The renderer
    // now projects the *logical* margin and the physical one is pagination's
    // alone. This check reads the computed margin *after* pagination has had
    // its say, which is the only place the old behaviour was visible.
    await openBlankDocument(chrome);
    await chrome.type("spaced paragraph");
    await settle(chrome);
    await chrome.evaluate(() => {
      const select = document.querySelector('[data-toolbar] select[data-select="space-before"]');
      const option = Array.from(select.options).find((item) => item.textContent === "18 pt");
      if (!option) {
        throw new Error(`no 18 pt option: ${Array.from(select.options, (o) => o.textContent).join(", ")}`);
      }
      select.value = option.value;
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await poll("the spacing in the markup", () =>
      chrome.evaluate(() => {
        const block = document.querySelector('[contenteditable="true"] [data-block-id]');
        return !!block && (block.getAttribute("style") ?? "").includes("margin-block-start");
      }),
    );
    // Let pagination run over the new markup — it is a round trip through the
    // core, and clearing the margin was the last thing it did.
    await settle(chrome);
    await settle(chrome);
    await settle(chrome);
    const spacing = await chrome.evaluate(() => {
      const block = document.querySelector('[contenteditable="true"] [data-block-id]');
      return {
        margin: window.getComputedStyle(block).marginTop,
        paginated: block.dataset.pageIndex ?? null,
      };
    });
    assert.equal(spacing.paginated, "0", "pagination had not run, so this proves nothing");
    assert.ok(
      Math.abs(parseFloat(spacing.margin) - 24) < 0.5,
      `18pt of space before computed to ${spacing.margin} after pagination, expected 24px`,
    );
  });

  await check("a page-break margin is cleared when the break moves off a block", async () => {
    // `editor.ts` leaves a block's DOM alone when its markup did not change,
    // and changing the page size changes no block's markup at all — the body
    // projection does not know about pages. So the only thing that can take a
    // page break off a block that no longer opens a page is pagination
    // writing the placement of *every* block, and this check fails if it ever
    // writes only the placements it was given.
    await openBlankDocument(chrome);
    await menuAction("File", "page-setup");
    await poll("the page setup prompt", () =>
      answerPrompt("Page setup", {
        size: "custom",
        orientation: "landscape",
        width: "8.50",
        height: "3.00",
        top: "0.50",
        bottom: "0.50",
        start: "0.50",
        end: "0.50",
      }),
    );
    await poll("the short page", async () => {
      const measured = await sheetGeometry();
      return measured && measured.height === 288 ? measured : null;
    });
    for (let i = 0; i < 14; i += 1) {
      await chrome.type(`line ${i}`);
      await chrome.press("Enter");
    }
    const broken = await poll("a block opening page 2", () =>
      chrome.evaluate(() => {
        const blocks = [...document.querySelectorAll('[contenteditable="true"] [data-block-id]')];
        const opener = blocks.find((block) => block.dataset.pageIndex === "1" && block.style.marginTop);
        if (!opener) return null;
        return {
          id: opener.dataset.blockId,
          margin: window.getComputedStyle(opener).marginTop,
          text: opener.textContent,
        };
      }),
    );
    assert.ok(
      parseFloat(broken.margin) > 10,
      `the block opening page 2 carried ${broken.margin}, so there was no break to clear`,
    );
    // Back to a page tall enough for everything, without touching the text.
    await menuAction("File", "page-setup");
    await poll("the page setup prompt", () =>
      answerPrompt("Page setup", {
        size: "letter",
        orientation: "portrait",
        top: "1.00",
        bottom: "1.00",
        start: "1.00",
        end: "1.00",
      }),
    );
    const settled = await poll("one page again", () =>
      chrome.evaluate((id) => {
        if (document.querySelectorAll(".page-sheet").length !== 1) return null;
        const block = document.querySelector(`[data-block-id="${id}"]`);
        if (!block) return null;
        return {
          page: block.dataset.pageIndex ?? null,
          margin: window.getComputedStyle(block).marginTop,
          text: block.textContent,
        };
      }, broken.id),
    );
    assert.equal(
      settled.text,
      broken.text,
      "the block's own markup changed, so this says nothing about placement",
    );
    assert.equal(settled.page, "0", `the block still claims page ${settled.page}`);
    assert.equal(
      parseFloat(settled.margin),
      0,
      `a stale page break left ${settled.margin} on a block that is now on page 1`,
    );
  });

  await check("the bundled document faces are the ones Chrome draws with", async () => {
    // The whole design rests on Rust measuring the font the browser renders.
    // If the WOFF2 failed to load, Chrome silently falls back to Arial and
    // every metric below becomes a metric for a document nobody sees — so the
    // failure has to name that cause rather than showing up as a stray pixel.
    await openBlankDocument(chrome);
    await chrome.type("metrics");
    await settle(chrome);
    const fonts = await poll("the loaded document faces", () =>
      chrome.evaluate(async () => {
        const block = document.querySelector('[contenteditable="true"] [data-block-id]');
        if (!block) return null;
        // `load` resolves only once the file has been fetched and parsed, so
        // a missing or corrupt WOFF2 fails here rather than passing as a
        // declaration that nothing ever used.
        const faces = [
          '11pt "OpenDoc Sans"',
          'bold 11pt "OpenDoc Sans"',
          'italic 11pt "OpenDoc Sans"',
          'bold italic 11pt "OpenDoc Sans"',
          '11pt "OpenDoc Mono"',
        ];
        const loaded = await Promise.all(
          faces.map(async (face) => {
            const found = await document.fonts.load(face);
            return found.length > 0;
          }),
        );
        return {
          regular: loaded[0],
          bold: loaded[1],
          italic: loaded[2],
          boldItalic: loaded[3],
          mono: loaded[4],
          used: window.getComputedStyle(block).fontFamily,
        };
      }),
    );
    assert.equal(fonts.regular, true, "the regular document face did not load");
    assert.equal(fonts.bold, true, "the bold document face did not load");
    assert.equal(fonts.italic, true, "the italic document face did not load");
    assert.equal(fonts.boldItalic, true, "the bold italic document face did not load");
    assert.equal(fonts.mono, true, "the monospace document face did not load");
    assert.ok(
      fonts.used.includes("OpenDoc Sans"),
      `the page asked for ${JSON.stringify(fonts.used)}, not the bundled family`,
    );
  });

  await check("Rust's page assignment matches where Chrome drew each block", async () => {
    await openBlankDocument(chrome);
    // A short page plus enough content to cross several of them. Every block
    // carries `data-page-index`, which is Rust's own answer; the geometry is
    // Chrome's. Any disagreement in a line count anywhere in the flow
    // accumulates and pushes a later block out of the page it was assigned,
    // so this is sensitive to exactly the mismatch that would matter.
    await menuAction("File", "page-setup");
    await poll("the page setup prompt", () =>
      answerPrompt("Page setup", {
        size: "custom",
        orientation: "landscape",
        width: "8.50",
        height: "3.00",
        top: "0.50",
        bottom: "0.50",
        start: "0.50",
        end: "0.50",
      }),
    );
    await poll("the short page", async () => {
      const measured = await sheetGeometry();
      return measured && measured.height === 288 ? measured : null;
    });
    // Mixed content: text that has to wrap (so the line count is a real
    // measurement, not a constant), a heading with its own size and space
    // above it, and a bulleted list with its own indent.
    const lines = [
      "The quick brown fox jumps over the lazy dog, and then keeps running until the line has to wrap somewhere.",
      "Short one.",
      "Another paragraph long enough that it wraps at least once inside a seven and a half inch column of eleven point text.",
    ];
    for (let i = 0; i < 9; i += 1) {
      await chrome.type(lines[i % lines.length]);
      await chrome.press("Enter");
    }
    await chrome.type("A heading");
    await settle(chrome);
    await menuAction("Format", "style:heading:1");
    await settle(chrome);
    await chrome.press("End");
    await chrome.press("Enter");
    await menuAction("Format", "style:paragraph");
    await settle(chrome);
    for (let i = 0; i < 6; i += 1) {
      await chrome.type(`tail paragraph ${i} with a little more text on it so that it is not trivially short`);
      await chrome.press("Enter");
    }
    await settle(chrome);

    const agreement = await poll("a paginated multi-page document", () =>
      chrome.evaluate(() => {
        const sheets = [...document.querySelectorAll(".page-sheet")].map((sheet) =>
          sheet.getBoundingClientRect(),
        );
        const blocks = [
          ...document.querySelectorAll('[contenteditable="true"] [data-block-id][data-page-index]'),
        ];
        const flow = document.querySelector("[data-page]");
        if (sheets.length < 2 || blocks.length === 0 || !flow) return null;
        const style = window.getComputedStyle(flow);
        const padTop = parseFloat(style.paddingTop);
        const padBottom = parseFloat(style.paddingBottom);
        const disagreements = [];
        for (const block of blocks) {
          const page = Number(block.dataset.pageIndex);
          const sheet = sheets[page];
          const rect = block.getBoundingClientRect();
          if (!sheet) {
            disagreements.push({ id: block.dataset.blockId, page, reason: "no such page drawn" });
            continue;
          }
          const top = sheet.top + padTop;
          const bottom = sheet.bottom - padBottom;
          // Half a pixel of slack: Chrome lays out on a 1/64px grid and the
          // sheet's own top is a rounded pt value. Anything larger than that
          // is a real disagreement, not a rounding artefact.
          if (rect.top < top - 0.5) {
            disagreements.push({
              id: block.dataset.blockId,
              page,
              reason: `drawn ${(top - rect.top).toFixed(2)}px above page ${page}'s content box`,
            });
          } else if (rect.bottom > bottom + 0.5 && rect.height <= bottom - top) {
            disagreements.push({
              id: block.dataset.blockId,
              page,
              reason: `drawn ${(rect.bottom - bottom).toFixed(2)}px below page ${page}'s content box`,
            });
          }
        }
        // A block never overflowing its page is only half of "the break is in
        // the right place": breaking a page early would also satisfy it. So
        // check the other direction too — the block that opens a page must
        // not have fitted at the bottom of the page before it. Its own margin
        // is ignored, which makes the test lenient rather than flaky: it only
        // complains when the block would have fitted with room to spare.
        for (let page = 1; page < sheets.length; page += 1) {
          const onPage = blocks.filter((block) => Number(block.dataset.pageIndex) === page);
          const onPrevious = blocks.filter((block) => Number(block.dataset.pageIndex) === page - 1);
          if (onPage.length === 0 || onPrevious.length === 0) continue;
          const opener = onPage[0];
          // An explicit page break is a document instruction, not an overflow.
          if (opener.previousElementSibling?.classList.contains("doc-page-break")) continue;
          const last = onPrevious[onPrevious.length - 1].getBoundingClientRect();
          const height = opener.getBoundingClientRect().height;
          const room = sheets[page - 1].bottom - padBottom - last.bottom;
          if (height <= room - 0.5) {
            disagreements.push({
              id: opener.dataset.blockId,
              page,
              reason: `opened page ${page} although ${height.toFixed(2)}px fitted in the ${room.toFixed(2)}px left on page ${page - 1}`,
            });
          }
        }
        return { pages: sheets.length, blocks: blocks.length, disagreements };
      }),
    );
    assert.ok(agreement.pages >= 3, `expected several pages, got ${agreement.pages}`);
    assert.ok(agreement.blocks >= 15, `only ${agreement.blocks} blocks carried a page assignment`);
    assert.deepEqual(
      agreement.disagreements,
      [],
      `Rust's pagination disagrees with what Chrome drew: ${JSON.stringify(agreement.disagreements)}`,
    );
  });

  await check("the type scale Rust measured with is the one the page draws", async () => {
    // The other half of the loop. Rust computes a heading's height from its
    // own type scale; if the stylesheet used a different size the heights
    // would drift silently, so the scale is projected and the stylesheet
    // reads it. This asserts the projection actually reaches Chrome.
    const scale = await poll("the projected type scale", () =>
      chrome.evaluate(() => {
        const rule = document.getElementById("doc-type-scale")?.textContent || null;
        const heading = document.querySelector('[contenteditable="true"] h1[data-block-id]');
        const flow = document.querySelector("[data-page]");
        if (!rule || !heading || !flow) return null;
        const root = window.getComputedStyle(document.documentElement);
        return {
          rule,
          declaredH1: root.getPropertyValue("--doc-h1-size").trim(),
          headingSize: window.getComputedStyle(heading).fontSize,
          bodySize: window.getComputedStyle(flow).fontSize,
          lineHeight: window.getComputedStyle(flow).lineHeight,
        };
      }),
    );
    assert.ok(scale.rule.includes("--doc-h1-size"), "the type scale rule was not projected");
    assert.equal(scale.declaredH1, "20pt", `--doc-h1-size resolved to ${scale.declaredH1}`);
    // 20pt is 26.6667px, 11pt is 14.6667px, and 1.5 of that is 22px.
    assert.ok(
      Math.abs(parseFloat(scale.headingSize) - 80 / 3) < 0.05,
      `the heading computed to ${scale.headingSize}, not the 20pt the layout measured`,
    );
    assert.ok(
      Math.abs(parseFloat(scale.bodySize) - 44 / 3) < 0.05,
      `body text computed to ${scale.bodySize}, not the 11pt the layout measured`,
    );
    assert.ok(
      Math.abs(parseFloat(scale.lineHeight) - 22) < 0.05,
      `the leading computed to ${scale.lineHeight}, not the 22px the layout measured`,
    );
  });

  await check("the print box carries the document's own page size", async () => {
    // Geometry-independent: the rule must name the same sheet the page stack
    // drew, in pt. `@page` cannot read custom properties, so this is projected
    // in Rust and has to be a real length, not a var().
    const projected = await poll("the projected @page rule", () =>
      chrome.evaluate(() => {
        const rule = document.getElementById("page-print-style")?.textContent || null;
        const sheet = document.querySelector(".page-sheet");
        if (!rule || !sheet) return null;
        const rect = sheet.getBoundingClientRect();
        return { rule, width: rect.width, height: rect.height };
      }),
    );
    const match = /^@page \{ size: ([\d.]+)pt ([\d.]+)pt; margin: 0; \}$/.exec(projected.rule);
    assert.ok(match, `@page rule was ${JSON.stringify(projected.rule)}`);
    // 1pt is 4/3 CSS px by definition.
    assert.ok(
      Math.abs(Number(match[1]) * (4 / 3) - projected.width) <= 1,
      `@page width ${match[1]}pt does not match the ${projected.width}px sheet`,
    );
    assert.ok(
      Math.abs(Number(match[2]) * (4 / 3) - projected.height) <= 1,
      `@page height ${match[2]}pt does not match the ${projected.height}px sheet`,
    );
  });

  // ---- Images: drop, resize, placement (PLAN77 E3 / OB-17, OB-18) --------
  //
  // jsdom structurally cannot make these: dropping a file needs a real
  // DataTransfer, and every assertion below is a *computed* box or style,
  // which needs a layout engine. The picture is an SVG because it is the one
  // image format that can be written as text here and still gives Chrome a
  // real intrinsic size to resize away from.
  const DROPPED_SVG =
    '<svg xmlns="http://www.w3.org/2000/svg" width="120" height="60"><rect width="120" height="60" fill="#4285f4"/></svg>';

  await check("a dropped image file becomes an image block", async () => {
    await openBlankDocument(chrome);
    await chrome.evaluate((svg) => {
      const host = document.querySelector('[contenteditable="true"]');
      const block = host.querySelector("[data-block-id]");
      const rect = block.getBoundingClientRect();
      const transfer = new DataTransfer();
      transfer.items.add(new File([svg], "dropped.svg", { type: "image/svg+xml" }));
      const at = { clientX: rect.left + 4, clientY: rect.top + 4, bubbles: true, cancelable: true, dataTransfer: transfer };
      host.dispatchEvent(new DragEvent("dragover", at));
      host.dispatchEvent(new DragEvent("drop", at));
    }, DROPPED_SVG);
    const image = await poll("the dropped image", () =>
      chrome.evaluate(() => {
        const img = document.querySelector('[contenteditable="true"] figure.doc-image img');
        if (!img) return null;
        const rect = img.getBoundingClientRect();
        return {
          blockId: img.closest("[data-block-id]").getAttribute("data-block-id"),
          placement: img.closest("figure").getAttribute("data-placement"),
          alt: img.getAttribute("alt"),
          title: img.getAttribute("title"),
          width: rect.width,
          blocks: document.querySelectorAll('[contenteditable="true"] figure.doc-image').length,
        };
      }),
    );
    assert.equal(image.blocks, 1, `the drop produced ${image.blocks} image blocks`);
    assert.equal(image.placement, "block", "a new image is placed on its own line");
    assert.equal(image.alt, "", "a dropped file name is not fabricated as alternative text");
    assert.equal(image.title, null, "a dropped file name is not projected as a hover title");
    // The picture is drawn at the size its bytes decode to, because the drop
    // stated no size: a materialised default would show up as anything else.
    assert.ok(
      Math.abs(image.width - 120) <= 1,
      `a freshly dropped image rendered ${image.width}px wide, not its intrinsic 120px`,
    );
  });

  await check("dragging an image's edge sets the width it computes to", async () => {
    const before = await chrome.evaluate(() => {
      const img = document.querySelector('[contenteditable="true"] figure.doc-image img');
      const rect = img.getBoundingClientRect();
      return { right: rect.right, top: rect.top, height: rect.height, width: rect.width };
    });
    // One mousedown on the trailing edge, moves, one mouseup — the gesture a
    // person makes. There are no handle elements to find: the hit zone is
    // computed, which is exactly what this proves.
    await chrome.evaluate((box) => {
      const img = document.querySelector('[contenteditable="true"] figure.doc-image img');
      const y = box.top + box.height / 2;
      const down = new MouseEvent("mousedown", { clientX: box.right - 2, clientY: y, bubbles: true, cancelable: true });
      img.dispatchEvent(down);
      for (const offset of [20, 40, 60]) {
        window.dispatchEvent(new MouseEvent("mousemove", { clientX: box.right - 2 + offset, clientY: y, bubbles: true }));
      }
      window.dispatchEvent(new MouseEvent("mouseup", { clientX: box.right + 58, clientY: y, bubbles: true }));
    }, before);
    await poll("the resized image", () =>
      chrome.evaluate((wanted) => {
        const img = document.querySelector('[contenteditable="true"] figure.doc-image img');
        const rect = img.getBoundingClientRect();
        return Math.abs(rect.width - wanted) <= 2 ? { width: rect.width } : null;
      }, before.width + 60),
    );
    // Now force a re-render out of the document. The morph rewrites the
    // picture's style attribute from what Rust projected, so a width that
    // survives this came from the document — not from the preview the drag
    // left on screen. Without this the check would pass on the preview alone,
    // which is exactly the mutation that proved it: dropping the command from
    // the mouse-up left this assertion green.
    await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const run = host.querySelector("[data-block-id] [data-inline-id]");
      const range = document.createRange();
      range.selectNodeContents(run);
      range.collapse(false);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.dispatchEvent(new Event("selectionchange"));
    });
    await settle(chrome);
    await chrome.type("!");
    await settle(chrome);
    const after = await poll("the stored size after a re-render", () =>
      chrome.evaluate((wanted) => {
        const img = document.querySelector('[contenteditable="true"] figure.doc-image img');
        const rect = img.getBoundingClientRect();
        return Math.abs(rect.width - wanted) <= 2 ? { width: rect.width, height: rect.height } : null;
      }, before.width + 60),
    );
    assert.ok(
      Math.abs(after.width - (before.width + 60)) <= 2,
      `the drag left the image ${after.width}px wide, not ${before.width + 60}px`,
    );
    // The height axis was never dragged, so it must have followed the width
    // rather than staying put — a stretched picture is the bug this catches.
    assert.ok(
      after.height > before.height + 10,
      `the height stayed at ${after.height}px while the width grew to ${after.width}px`,
    );
  });

  await check("wrapping an image floats it", async () => {
    // Put the caret on the image the way clicking it does, then use the app's
    // own menu item — the whole path a user takes, delegated listener and all.
    const blockId = await chrome.evaluate(() => {
      const figure = document.querySelector('[contenteditable="true"] figure.doc-image');
      const range = document.createRange();
      range.setStart(figure, 0);
      range.collapse(true);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.dispatchEvent(new Event("selectionchange"));
      return figure.getAttribute("data-block-id");
    });
    await settle(chrome);
    await chrome.evaluate(() => document.querySelector('[data-action="image-placement:wrap-end"]').click());
    const floated = await poll("the floated image", () =>
      chrome.evaluate((id) => {
        const figure = document.querySelector(`figure.doc-image[data-block-id="${id}"]`);
        if (!figure || figure.getAttribute("data-placement") !== "wrap-end") return null;
        return { float: getComputedStyle(figure).float };
      }, blockId),
    );
    assert.ok(
      floated.float === "right" || floated.float === "inline-end",
      `a wrapped image computed float: ${floated.float}`,
    );
  });

  await check("aligning an image moves it in its column", async () => {
    const blockId = await chrome.evaluate(() => {
      const figure = document.querySelector('[contenteditable="true"] figure.doc-image');
      const range = document.createRange();
      range.setStart(figure, 0);
      range.collapse(true);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.dispatchEvent(new Event("selectionchange"));
      return figure.getAttribute("data-block-id");
    });
    await settle(chrome);
    await chrome.evaluate(() => document.querySelector('[data-action="image-placement:block"]').click());
    await settle(chrome);
    await chrome.evaluate(() => document.querySelector('[data-action="align:end"]').click());
    const aligned = await poll("the aligned image", () =>
      chrome.evaluate((id) => {
        const figure = document.querySelector(`figure.doc-image[data-block-id="${id}"]`);
        if (!figure) return null;
        const style = getComputedStyle(figure);
        if (style.float !== "none") return null;
        // Only settled once the alignment has actually arrived: the default
        // figure style centres, so "center" means the command has not landed.
        const align = style.textAlign;
        return align === "end" || align === "right" ? { align } : null;
      }, blockId),
    );
    assert.ok(
      aligned.align === "end" || aligned.align === "right",
      `an image aligned right computed text-align: ${aligned.align}`,
    );
  });

  // ---- An image can be deleted (B3) --------------------------------------
  //
  // Found by driving a real browser and not otherwise findable: **a mouse
  // click on a `contenteditable="false"` figure does not move Chrome's
  // selection at all.** It is not anchored on the figure, not on the host,
  // not cleared — it stays exactly where it already was, even though
  // `document.caretRangeFromPoint` at those same coordinates answers
  // `(figure, 0)`. So an inserted image could not be reached by any gesture:
  // neither delete key ever saw it, and every Format ▸ Image menu item, each
  // of which reads `state.selection.focus.block_id`, answered "Select an
  // image first." however hard the image was clicked.
  //
  // `DocumentEditor.selectAtomicBlock` now selects the whole figure on click,
  // which is what these drive. jsdom cannot make this assertion: it has no
  // hit testing, so a synthetic click there proves nothing about what Chrome
  // does with a real one.

  /** Drops `DROPPED_SVG` into a fresh document and returns the figure's id. */
  const documentWithAnImage = async () => {
    await openBlankDocument(chrome);
    await chrome.type("before");
    await chrome.press("Enter");
    await chrome.type("after");
    await settle(chrome);
    // Dropped onto the first paragraph, so the picture lands between the two
    // and both neighbouring-paragraph gestures have somewhere to start.
    await chrome.evaluate((svg) => {
      const host = document.querySelector('[contenteditable="true"]');
      const block = host.querySelector("[data-block-id]");
      const rect = block.getBoundingClientRect();
      const transfer = new DataTransfer();
      transfer.items.add(new File([svg], "dropped.svg", { type: "image/svg+xml" }));
      const at = { clientX: rect.left + 4, clientY: rect.top + 4, bubbles: true, cancelable: true, dataTransfer: transfer };
      host.dispatchEvent(new DragEvent("dragover", at));
      host.dispatchEvent(new DragEvent("drop", at));
    }, DROPPED_SVG);
    return poll("the dropped image", () =>
      chrome.evaluate(() => {
        const figure = document.querySelector('[contenteditable="true"] figure.doc-image');
        return figure ? figure.getAttribute("data-block-id") : null;
      }),
    );
  };

  const figureCount = () =>
    chrome.evaluate(() => document.querySelectorAll('[contenteditable="true"] figure.doc-image').length);

  await check("clicking an image selects it, and Backspace then deletes it", async () => {
    await documentWithAnImage();
    assert.equal(await figureCount(), 1, "the image did not land");
    // A real mouse click, through Chrome's own hit testing.
    await chrome.click('[contenteditable="true"] figure.doc-image img');
    await settle(chrome);
    const selected = await chrome.evaluate(() => {
      const selection = window.getSelection();
      if (!selection.rangeCount) return null;
      const range = selection.getRangeAt(0);
      const figure = document.querySelector('[contenteditable="true"] figure.doc-image');
      return {
        // The whole object is selected: the range brackets the figure.
        containsFigure: range.intersectsNode(figure) && !selection.isCollapsed,
      };
    });
    assert.ok(selected?.containsFigure, "clicking the image did not select it");
    await chrome.press("Backspace");
    await settle(chrome);
    assert.equal(await figureCount(), 0, "Backspace did not delete the selected image");
  });

  await check("clicking an image and pressing Delete deletes it too", async () => {
    await documentWithAnImage();
    await chrome.click('[contenteditable="true"] figure.doc-image img');
    await settle(chrome);
    await chrome.press("Delete");
    await settle(chrome);
    assert.equal(await figureCount(), 0, "Delete did not delete the selected image");
  });

  await check("Backspace at the start of the paragraph after an image deletes the image", async () => {
    await documentWithAnImage();
    // The caret goes where a user would put it: the start of the paragraph
    // the drop left after the picture.
    await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const blocks = Array.from(host.querySelectorAll(":scope > [data-block-id]"));
      const figure = blocks.findIndex((el) => el.getAttribute("data-kind") === "image");
      const paragraph = blocks.slice(figure + 1).find((el) => el.getAttribute("data-kind") === "paragraph");
      const range = document.createRange();
      range.selectNodeContents(paragraph.querySelector("[data-inline-id]") ?? paragraph);
      range.collapse(true);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.dispatchEvent(new Event("selectionchange"));
    });
    await settle(chrome);
    await chrome.press("Backspace");
    await settle(chrome);
    assert.equal(await figureCount(), 0, "Backspace before the image left it there");
  });

  // ---- The editable host is sized by the page, not by a constant ---------
  //
  // `.doc-body` carried a flat `min-height: 800px` — taller than the content
  // box of any page shorter than about nine inches, so on a short page the
  // editable host stuck out below the page it belongs to. Nothing is painted
  // there, which is why it survived; only a measurement catches it.
  await check("the editable host is as tall as the page's content box", async () => {
    await openBlankDocument(chrome);
    await menuAction("File", "page-setup");
    await poll("the page setup prompt", () =>
      answerPrompt("Page setup", {
        size: "custom",
        orientation: "landscape",
        width: "8.50",
        height: "3.00",
        top: "0.50",
        bottom: "0.50",
        start: "0.50",
        end: "0.50",
      }),
    );
    await poll("the short page", async () => {
      const measured = await sheetGeometry();
      return measured && measured.height === 288 ? measured : null;
    });
    const measured = await poll("the editable host on a short page", () =>
      chrome.evaluate(() => {
        const host = document.querySelector('[contenteditable="true"]');
        const flow = document.querySelector("[data-page]");
        if (!host || !flow) return null;
        const padding = window.getComputedStyle(flow);
        return {
          // 3in minus two half-inch margins is 2in is 192 CSS px.
          minHeight: Math.round(parseFloat(window.getComputedStyle(host).minHeight)),
          contentHeight: Math.round(
            flow.getBoundingClientRect().height -
              parseFloat(padding.paddingTop) -
              parseFloat(padding.paddingBottom),
          ),
        };
      }),
    );
    assert.equal(
      measured.minHeight,
      192,
      `the editable host asks for ${measured.minHeight}px on a page whose content box is 192px`,
    );
    assert.ok(
      measured.minHeight <= measured.contentHeight,
      `the editable host (${measured.minHeight}px) is taller than the flow's content box (${measured.contentHeight}px)`,
    );
  });

  // ---- Tables: geometry Chrome computes, not markup jsdom accepts --------
  // These are the assertions the file's opening comment is about: a column
  // width that reaches `getComputedStyle`, and a merged cell that really
  // covers its neighbours' boxes. (PLAN77 E2, ADR 0013.)

  /** Waits for the named prompt, fills it in and submits it. */
  async function answerNamedPrompt(title, values) {
    await poll(`${title} prompt`, () => answerPrompt(title, values), 60, 100);
    await poll(
      `${title} prompt closed`,
      () => chrome.evaluate(() => !document.querySelector("dialog.modal[open]")),
      60,
      100,
    );
  }

  /** Puts the caret in the table cell at (row, column) of the first table. */
  async function caretInCell(row, column) {
    await chrome.evaluate(
      (at) => {
        const table = document.querySelector('[contenteditable="true"] table[data-block-id]');
        const cell = table.querySelectorAll("tbody > tr")[at.row].querySelectorAll("td")[at.column];
        const run = cell.querySelector("[data-inline-id]") ?? cell;
        const range = document.createRange();
        range.selectNodeContents(run);
        range.collapse(false);
        const selection = window.getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
        document.querySelector('[contenteditable="true"]').focus();
      },
      { row, column },
    );
    await settle(chrome);
  }

  /**
   * Selects the rectangle of cells from (fromRow, fromColumn) to
   * (toRow, toColumn) — the gesture a drag across a table makes, which is what
   * "Merge cells" now reads instead of asking for a span in a dialog.
   */
  async function selectCells(fromRow, fromColumn, toRow, toColumn) {
    await chrome.evaluate(
      (at) => {
        const table = document.querySelector('[contenteditable="true"] table[data-block-id]');
        const cellAt = (row, column) =>
          table.querySelectorAll("tbody > tr")[row].querySelectorAll("td")[column];
        const start = cellAt(at.fromRow, at.fromColumn);
        const end = cellAt(at.toRow, at.toColumn);
        const range = document.createRange();
        range.setStart(start, 0);
        range.setEnd(end, end.childNodes.length);
        const selection = window.getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
        document.querySelector('[contenteditable="true"]').focus();
      },
      { fromRow, fromColumn, toRow, toColumn },
    );
    await settle(chrome);
  }

  async function insertTable(rows, columns) {
    await openBlankDocument(chrome);
    await chrome.type("before the table");
    await settle(chrome);
    await menuAction("Insert", "insert-table");
    await answerNamedPrompt("Insert table", { rows: String(rows), columns: String(columns) });
    await chrome.waitUntil(
      "table",
      () => !!document.querySelector('[contenteditable="true"] table[data-block-id] td'),
      { tries: 60, delayMs: 100 },
    );
  }

  await check("a column width set in points computes to exactly that width", async () => {
    await insertTable(2, 3);
    await caretInCell(0, 1);
    await menuAction("Table", "table-column-width");
    // 144pt = 2in = exactly 192 CSS px. A layout that scales specified widths
    // to fit — the bug this suite exists for — lands on something else.
    await answerNamedPrompt("Column width", { points: "144" });
    // Wait on the *markup* reaching the page — a <col> that carries a width —
    // and then assert on the *layout* Chrome computed from it. Waiting on the
    // computed width instead would make the assertion tautological.
    const widths = await chrome.waitUntil(
      "resized column",
      () => {
        const table = document.querySelector('[contenteditable="true"] table[data-block-id]');
        const columns = table?.querySelectorAll("colgroup > col");
        const cells = table?.querySelectorAll("tbody > tr")[0]?.querySelectorAll("td");
        if (!columns || columns.length !== 3 || !cells || cells.length !== 3) return null;
        if (!columns[1].getAttribute("style")) return null;
        return Array.from(cells, (cell) => cell.getBoundingClientRect().width);
      },
      { tries: 60, delayMs: 100 },
    );
    assert.ok(
      Math.abs(widths[1] - 192) < 1.5,
      `the 2in column computed to ${widths[1]}px, expected 192px`,
    );

    await menuAction("Table", "table-column-width-auto");
    const auto = await chrome.waitUntil(
      "auto column",
      () => {
        const table = document.querySelector('[contenteditable="true"] table[data-block-id]');
        const columns = table?.querySelectorAll("colgroup > col");
        const cells = table?.querySelectorAll("tbody > tr")[0]?.querySelectorAll("td");
        if (!columns || !cells || columns[1].getAttribute("style")) return null;
        return cells[1].getBoundingClientRect().width;
      },
      { tries: 60, delayMs: 100 },
    );
    assert.ok(
      Math.abs(auto - 192) > 2,
      `clearing the width left the column at ${auto}px, still the 2in it was set to`,
    );
  });

  await check("a merged cell covers its neighbours' boxes", async () => {
    await insertTable(2, 3);
    const before = await chrome.evaluate(() => {
      const table = document.querySelector('[contenteditable="true"] table[data-block-id]');
      return table.querySelectorAll("td").length;
    });
    assert.equal(before, 6, "the inserted table is 2x3");

    // The span is the selection, not a number typed into a dialog: what is
    // dragged across is what merges.
    await selectCells(0, 0, 1, 1);
    await menuAction("Table", "table-merge-cells");
    const merged = await chrome.waitUntil(
      "merged cell",
      () => {
        const table = document.querySelector('[contenteditable="true"] table[data-block-id]');
        const cells = table?.querySelectorAll("td");
        if (!cells || cells.length !== 3) return null;
        const first = cells[0];
        const box = first.getBoundingClientRect();
        const neighbour = cells[1].getBoundingClientRect();
        return {
          rowSpan: first.rowSpan,
          colSpan: first.colSpan,
          width: box.width,
          height: box.height,
          neighbourWidth: neighbour.width,
          neighbourHeight: neighbour.height,
        };
      },
      { tries: 60, delayMs: 100 },
    );
    // Three cells left of six, and the survivor really is two columns wide and
    // two rows tall in Chrome's own layout.
    assert.equal(merged.rowSpan, 2);
    assert.equal(merged.colSpan, 2);
    assert.ok(
      merged.width > merged.neighbourWidth * 1.5,
      `the merged cell is ${merged.width}px wide against a ${merged.neighbourWidth}px neighbour`,
    );
    assert.ok(
      merged.height > merged.neighbourHeight * 1.5,
      `the merged cell is ${merged.height}px tall against a ${merged.neighbourHeight}px neighbour`,
    );

    // Splitting hands the covered cells back, content and all.
    await caretInCell(0, 0);
    await menuAction("Table", "table-split-cell");
    const restored = await chrome.waitUntil(
      "split cell",
      () => {
        const table = document.querySelector('[contenteditable="true"] table[data-block-id]');
        const cells = table?.querySelectorAll("td");
        return cells && cells.length === 6 ? cells.length : null;
      },
      { tries: 60, delayMs: 100 },
    );
    assert.equal(restored, 6);
  });

  await check("cell blocks move by their real sibling relationship", async () => {
    await insertTable(1, 1);
    await caretInCell(0, 0);
    await chrome.type("first");
    await chrome.press("Enter");
    await chrome.type("second");
    await settle(chrome);

    await menuAction("Table", "table-move-cell-block-up");
    const order = await chrome.waitUntil(
      "reordered cell blocks",
      () => {
        const cell = document.querySelector('[contenteditable="true"] td[data-cell-id]');
        const blocks = Array.from(cell?.children ?? []).filter((child) => child.hasAttribute("data-block-id"));
        const text = blocks.map((block) => block.textContent?.trim());
        return text.join("|") === "second|first" ? text : null;
      },
      { tries: 60, delayMs: 100 },
    );
    assert.deepEqual(order, ["second", "first"]);
  });

  await check("cell block move uses model siblings across a list wrapper", async () => {
    await insertTable(1, 1);
    await caretInCell(0, 0);
    await chrome.type("paragraph");
    await chrome.press("Enter");
    await chrome.type("list item");
    await settle(chrome);
    await menuAction("Format", "style:list:bullet");
    await settle(chrome);
    assert.equal(
      await chrome.evaluate(() => document.querySelectorAll('[contenteditable="true"] td[data-cell-id] ul li[data-block-id]').length),
      1,
      "the cell fixture did not place the focused block under a list wrapper",
    );

    await menuAction("Table", "table-move-cell-block-up");
    const order = await chrome.waitUntil(
      "list item moved before its paragraph sibling",
      () => {
        const cell = document.querySelector('[contenteditable="true"] td[data-cell-id]');
        const blocks = Array.from(cell?.querySelectorAll('[data-block-id]') ?? [])
          .filter((block) => block.closest('td[data-cell-id], th[data-cell-id]') === cell);
        const text = blocks.map((block) => block.textContent?.trim());
        return text.join("|") === "list item|paragraph" ? text : null;
      },
      { tries: 60, delayMs: 100 },
    );
    assert.deepEqual(order, ["list item", "paragraph"]);
  });

  await check("inserting and deleting a column keeps every row the same length", async () => {
    await insertTable(2, 3);
    await caretInCell(0, 0);
    await menuAction("Table", "table-insert-column-right");
    const widened = await chrome.waitUntil(
      "wider table",
      () => {
        const table = document.querySelector('[contenteditable="true"] table[data-block-id]');
        const rows = table?.querySelectorAll("tbody > tr");
        if (!rows) return null;
        const lengths = Array.from(rows, (row) => row.querySelectorAll("td").length);
        return lengths.every((length) => length === 4) ? lengths : null;
      },
      { tries: 60, delayMs: 100 },
    );
    assert.deepEqual(widened, [4, 4]);

    await caretInCell(0, 0);
    await menuAction("Table", "table-delete-column");
    const narrowed = await chrome.waitUntil(
      "narrower table",
      () => {
        const table = document.querySelector('[contenteditable="true"] table[data-block-id]');
        const rows = table?.querySelectorAll("tbody > tr");
        if (!rows) return null;
        const lengths = Array.from(rows, (row) => row.querySelectorAll("td").length);
        return lengths.every((length) => length === 3) ? lengths : null;
      },
      { tries: 60, delayMs: 100 },
    );
    assert.deepEqual(narrowed, [3, 3]);
  });

  await check("a cell background reaches its computed style", async () => {
    await insertTable(2, 2);
    await caretInCell(1, 1);
    await menuAction("Table", "table-cell-background");
    await answerNamedPrompt("Cell background", { color: "#ffee00" });
    const background = await chrome.waitUntil(
      "styled cell",
      () => {
        const table = document.querySelector('[contenteditable="true"] table[data-block-id]');
        const cell = table?.querySelectorAll("tbody > tr")[1]?.querySelectorAll("td")[1];
        if (!cell) return null;
        const colour = window.getComputedStyle(cell).backgroundColor;
        return colour === "rgba(0, 0, 0, 0)" ? null : colour;
      },
      { tries: 60, delayMs: 100 },
    );
    assert.equal(background, "rgb(255, 238, 0)");
  });

  // ---- Export (PLAN77 D1 / parity FS-21, UI-16) --------------------------
  //
  // The PDF is drawn from `opendoc-layout`, the same pass that decides where
  // the browser breaks its pages (ADR 0014). The check that matters is
  // therefore the same one the pagination section makes: *agreement*. These
  // capture what the real save path actually hands the user — the command
  // runs in the real WASM core and the payload is read back out of the
  // download — and compare it against the page Chrome computed.

  /** Runs a File-menu download and returns what `saveFile` handed the user. */
  async function captureDownload(action) {
    await chrome.evaluate(() => {
      window.__downloads = [];
      if (!window.__clickPatched) {
        const original = HTMLAnchorElement.prototype.click;
        HTMLAnchorElement.prototype.click = function patched() {
          if (this.download) {
            window.__downloads.push({ href: this.href, name: this.download });
            return;
          }
          return original.call(this);
        };
        window.__clickPatched = true;
      }
    });
    await menuAction("File", action);
    return poll("a download", () =>
      chrome.evaluate(async () => {
        const entry = window.__downloads?.[0];
        if (!entry) return null;
        // A data: URL carries base64 bytes; a blob: URL carries text.
        const response = await fetch(entry.href);
        const body = entry.href.startsWith("data:")
          ? [...new Uint8Array(await response.arrayBuffer())]
              .map((byte) => String.fromCharCode(byte))
              .join("")
          : await response.text();
        return { name: entry.name, href: entry.href.slice(0, 64), body };
      }),
    );
  }

  await check("the PDF download has the page box and the page count Chrome drew", async () => {
    await openBlankDocument(chrome);
    for (let i = 0; i < 6; i += 1) {
      await chrome.type(`exported line ${i}`);
      await chrome.press("Enter");
    }
    await settle(chrome);
    const geometry = await poll("a drawn page sheet", async () => await sheetGeometry());
    const download = await captureDownload("export-pdf");
    assert.ok(download.name.endsWith(".pdf"), `saved as ${download.name}`);
    assert.ok(
      download.href.startsWith("data:application/pdf;base64,"),
      `the media type came from the frontend, not the exporter: ${download.href}`,
    );
    assert.ok(download.body.startsWith("%PDF-"), "the payload is not a PDF");
    // Chrome laid the sheet out in CSS pixels; a CSS pixel is exactly 0.75pt,
    // and the PDF's MediaBox is in points. If the two disagreed, the PDF
    // would be a different piece of paper from the one on screen.
    const media = /\/MediaBox \[0 0 ([0-9.]+) ([0-9.]+)\]/.exec(download.body);
    assert.ok(media, "the PDF states no MediaBox");
    assert.equal(Math.round(Number(media[1])), Math.round(geometry.width * 0.75));
    assert.equal(Math.round(Number(media[2])), Math.round(geometry.height * 0.75));
    const pages = download.body.split("/Type /Page\n").length - 1;
    assert.equal(
      pages,
      geometry.sheets,
      `the PDF has ${pages} page(s) and Chrome drew ${geometry.sheets} sheet(s)`,
    );
  });

  await check("the HTML download carries the projected type scale", async () => {
    // The old TypeScript export pasted a hand-written stylesheet that said
    // `font-size: 11pt` on its own authority. The Rust export projects the
    // scale the layout engine measures with, so the custom properties below
    // are the evidence that the port actually happened.
    const download = await captureDownload("export-html");
    assert.ok(download.name.endsWith(".html"), `saved as ${download.name}`);
    assert.ok(download.body.startsWith("<!doctype html>"), download.body.slice(0, 60));
    assert.ok(download.body.includes("--doc-h1-size"), "the type scale is not projected");
    assert.ok(download.body.includes("--page-width"), "the page geometry is not projected");
    assert.ok(download.body.includes("exported line 0"), "the body markup is missing");
  });

  await check("a row inserted above the first one lands above it", async () => {
    // `after: None` means *append* everywhere in the operation log, so this
    // position needed a keyword of its own (`APP_INSERT_FIRST`) rather than a
    // second optional anchor. Before that it could not be expressed at all.
    await insertTable(2, 2);
    await caretInCell(0, 0);
    await chrome.type("top-left");
    await settle(chrome);
    await caretInCell(0, 0);
    await menuAction("Table", "table-insert-row-above");
    const rows = await chrome.waitUntil(
      "taller table",
      () => {
        const table = document.querySelector('[contenteditable="true"] table[data-block-id]');
        const bodyRows = table?.querySelectorAll("tbody > tr");
        if (!bodyRows || bodyRows.length !== 3) return null;
        return Array.from(bodyRows, (row) => row.textContent.trim());
      },
      { tries: 60, delayMs: 100 },
    );
    assert.equal(rows.length, 3);
    assert.equal(rows[0], "", "the new row is empty and above the old first row");
    assert.ok(rows[1].includes("top-left"), `the old first row moved down: ${JSON.stringify(rows)}`);
  });

  await check("hiding rows and columns takes them out of the grid Chrome draws", async () => {
    await openBlankSpreadsheet(chrome);
    const focusGrid = () => chrome.evaluate(() => document.querySelector("[data-grid]").focus());
    const boxes = () =>
      chrome.evaluate(() => ({
        row2: document.querySelector('[data-grid] tr[data-row="2"]')?.getBoundingClientRect().height ?? -1,
        row3: document.querySelector('[data-grid] tr[data-row="3"]')?.getBoundingClientRect().height ?? -1,
        row4: document.querySelector('[data-grid] tr[data-row="4"]')?.getBoundingClientRect().height ?? -1,
        columnB: document.querySelector('[data-grid] th[data-column="B"]')?.getBoundingClientRect().width ?? -1,
        cellB1: document.querySelector('[data-grid] td[data-address="B1"]')?.getBoundingClientRect().width ?? -1,
      }));

    const before = await boxes();
    assert.ok(before.row2 > 0 && before.columnB > 0, `nothing is hidden yet: ${JSON.stringify(before)}`);

    await chrome.click('[data-grid] td[data-address="A2"]');
    await settle(chrome);
    await focusGrid();
    await chrome.press("ArrowDown", { modifiers: 8 });
    await settle(chrome);
    await menuAction("Insert", "hide-rows");
    // The renderer omits a hidden row, so the assertion is absence rather than
    // a zero height: `render_workbook_html` decides what the grid contains,
    // and `spreadsheet.ts` no longer strips anything after the morph.
    const hiddenRows = await chrome.waitUntil(
      "hidden rows",
      () => {
        const row = (label) => document.querySelector(`[data-grid] tr[data-row="${label}"]`);
        if (row("2") || row("3")) return null;
        const row4 = row("4");
        return row4 ? { row4: row4.getBoundingClientRect().height } : null;
      },
      { tries: 60, delayMs: 100 },
    );
    assert.ok(hiddenRows.row4 > 0, "an unrelated row was hidden too");

    await chrome.click('[data-grid] td[data-address="B1"]');
    await settle(chrome);
    await menuAction("Insert", "hide-columns");
    const hiddenColumn = await chrome.waitUntil(
      "hidden column",
      () => {
        const header = document.querySelector('[data-grid] th[data-column="B"]');
        const cell = document.querySelector('[data-grid] td[data-address="B1"]');
        const other = document.querySelector('[data-grid] th[data-column="C"]');
        if (header || cell || !other) return null;
        return {
          width: other.getBoundingClientRect().width,
          headers: document.querySelectorAll("[data-grid] th[data-column]").length,
          cells: document.querySelectorAll("[data-grid] tbody tr:first-child td").length,
        };
      },
      { tries: 60, delayMs: 100 },
    );
    assert.ok(hiddenColumn.width > 0, "hiding B took C with it");
    // A row still draws exactly one cell per drawn column: the omission has to
    // reach the header, the cells and the <colgroup> together or the grid
    // shears sideways.
    assert.equal(
      hiddenColumn.cells,
      hiddenColumn.headers,
      "a row no longer draws one cell per drawn column",
    );

    // Unhiding works by selecting across the gap: a hidden row is still inside
    // a range that spans it, which is the only gesture that can reach one.
    await chrome.click('[data-grid] td[data-address="A1"]');
    await settle(chrome);
    await focusGrid();
    for (let step = 0; step < 3; step += 1) await chrome.press("ArrowDown", { modifiers: 8 });
    await settle(chrome);
    await menuAction("Insert", "unhide-rows");
    const revealed = await chrome.waitUntil(
      "revealed rows",
      () => {
        const row2 = document.querySelector('[data-grid] tr[data-row="2"]');
        const row3 = document.querySelector('[data-grid] tr[data-row="3"]');
        if (!row2 || !row3) return null;
        const height = (row) => row.getBoundingClientRect().height;
        return height(row2) > 0 && height(row3) > 0 ? height(row2) : null;
      },
      { tries: 60, delayMs: 100 },
    );
    assert.ok(revealed > 0, "the rows came back");
  });

  // ---- Collaboration: two real clients, one real service ------------------
  //
  // CO-15 (transport) and CO-18 (presence). Everything here runs through the
  // shipped path: the page's own `connect`, the TypeScript socket, the Rust
  // driver in `opendoc-wasm`, and a real `opendoc-service` process over TCP.
  // `docs/adr/0018` is the design.

  await check("two browsers on one service document converge through the real shell path", async () => {
    const origin = new URL(server.url).origin;
    collabService = await startCollaborationService(origin);
    await openBlankDocument(chrome);
    const alice = await joinSession(chrome, {
      serviceUrl: collabService.url,
      subject: "alice",
      apiKey: ALICE_KEY,
      documentUuid: "",
      displayName: "Alice Anderson",
    });
    assert.ok(!alice?.error, `alice could not connect: ${JSON.stringify(alice)}`);
    await waitForPhase(chrome, "live", "alice's session goes live");
    const documentUuid = await chrome.evaluate(
      () => window.__OPENDOC_COLLAB__.status().session?.document_uuid ?? "",
    );
    assert.ok(documentUuid, "the welcome must carry the document the service created");

    // Only the owner can share, and only the service writes grants.
    const ownerToken = await serviceToken(collabService, "alice", ALICE_KEY);
    await grantRole(collabService, documentUuid, ownerToken, "bob", "editor");

    // A second browser, so a second IndexedDB: two tabs on one origin share a
    // store (ADR 0008) and would look like a collaboration bug.
    bobChrome = await launchChrome({ port: CDP_PORT + 1 });
    await bobChrome.goto(server.url);
    await openBlankDocument(bobChrome);
    const bob = await joinSession(bobChrome, {
      serviceUrl: collabService.url,
      subject: "bob",
      apiKey: BOB_KEY,
      documentUuid,
      displayName: "Bob Brown",
    });
    assert.ok(!bob?.error, `bob could not connect: ${JSON.stringify(bob)}`);
    await waitForPhase(bobChrome, "live", "bob's session goes live");

    // Alice types; the service makes it durable and fans it out; Bob's replica
    // re-merges from the base and Chrome draws it.
    await typeIntoDocument(chrome, "alice-typed");
    await waitForBodyText(bobChrome, "alice-typed", "bob sees what alice typed");
    // And back the other way, so the convergence is not one-directional.
    await typeIntoDocument(bobChrome, "bob-typed");
    await waitForBodyText(chrome, "bob-typed", "alice sees what bob typed");

    const aliceText = await bodyText(chrome);
    const bobText = await bodyText(bobChrome);
    assert.equal(
      aliceText,
      bobText,
      `the two replicas disagree:\nalice: ${JSON.stringify(aliceText)}\nbob:   ${JSON.stringify(bobText)}`,
    );
    assert.ok(aliceText.includes("alice-typed") && aliceText.includes("bob-typed"), aliceText);

    // Durability before acknowledgement: the service says "accepted" only once
    // the branch head names a manifest naming the segment. So an acknowledged
    // watermark above zero on both sides means both sides' work is on disk.
    const watermarks = await Promise.all([
      chrome.evaluate(() => window.__OPENDOC_COLLAB__.status().acknowledged_seq),
      bobChrome.evaluate(() => window.__OPENDOC_COLLAB__.status().acknowledged_seq),
    ]);
    assert.ok(watermarks[0] > 0 && watermarks[1] > 0, `nothing was acknowledged: ${watermarks}`);
  });

  await check("presence renders the other person, their role and their connections", async () => {
    // Peers come from `OpenDocPresencePeer` in the app's session, which only a
    // service presence frame writes.
    const chips = await pollUntil(
      chrome,
      "alice's peer list shows bob",
      () =>
        Array.from(document.querySelectorAll("[data-collab-peers] [data-collab-peer]")).length >= 2
          ? Array.from(document.querySelectorAll("[data-collab-peers] [data-collab-peer]")).map(
              (node) => ({ actor: node.dataset.collabPeer, text: node.innerText, title: node.title }),
            )
          : null,
      null,
    );
    const bobChip = chips.find((chip) => chip.actor === "actor-bob");
    assert.ok(bobChip, `bob is not in alice's presence region: ${JSON.stringify(chips)}`);
    assert.ok(bobChip.text.includes("Bob Brown"), `the chip must name him: ${bobChip.text}`);
    assert.ok(
      bobChip.title.includes("editor"),
      `the chip must carry the role the service granted: ${bobChip.title}`,
    );
    const aliceChip = chips.find((chip) => chip.actor === "actor-alice");
    assert.ok(aliceChip?.text.includes("(you)"), `this user must be marked: ${JSON.stringify(aliceChip)}`);

    // A caret is presence too, and it is relayed as an opaque anchor.
    const withCaret = await pollUntil(
      bobChrome,
      "bob sees where alice's caret is",
      () =>
        Array.from(document.querySelectorAll("[data-collab-peers] [data-collab-peer]")).find(
          (node) => node.dataset.collabPeer === "actor-alice" && node.title.includes("caret at"),
        )?.title ?? null,
      null,
    );
    assert.ok(withCaret.includes("caret at"), withCaret);

    // The opaque anchor is resolved locally after the document has rendered.
    // It is a body-level decoration rather than a child of contenteditable,
    // so a presence refresh cannot enter a copied selection or intercept an
    // edit gesture.
    const remoteCaret = await pollUntil(
      bobChrome,
      "bob paints alice's remote caret without changing the document DOM",
      () => {
        const caret = document.querySelector('[data-remote-caret="actor-alice"]');
        const overlay = document.querySelector("[data-remote-presence-overlay]");
        const body = document.querySelector(".doc-body");
        if (!caret || !overlay || !body) return null;
        const rect = caret.getBoundingClientRect();
        return {
          outsideEditable: !body.contains(caret),
          ariaHidden: overlay.getAttribute("aria-hidden"),
          pointerEvents: getComputedStyle(overlay).pointerEvents,
          visible: rect.height > 0,
        };
      },
      null,
    );
    assert.deepEqual(remoteCaret, {
      outsideEditable: true,
      ariaHidden: "true",
      pointerEvents: "none",
      visible: true,
    });

    // And the connection state is on screen in words, not only in an object.
    const pill = await chrome.evaluate(() => ({
      phase: document.querySelector("[data-collab-phase]")?.dataset.collabPhase,
      label: document.querySelector("[data-collab-phase]")?.textContent,
      role: document.querySelector("[data-collab-role]")?.textContent,
    }));
    assert.equal(pill.phase, "live");
    assert.equal(pill.label, "Live");
    assert.equal(pill.role, "owner", "alice created the document, so the service made her its owner");
  });

  await check("presence announcements are an accessible, persistent opt-out and do not speak muted departures", async () => {
    const documentUuid = await chrome.evaluate(
      () => window.__OPENDOC_COLLAB__.status().session?.document_uuid ?? "",
    );
    // Use the same pointer path a reader uses. Calling `.click()` from inside
    // an awaited Runtime.evaluate keeps that CDP evaluation alive while the
    // delegated handler replaces the control's parent; current Chrome can
    // then fail to settle the command despite completing the DOM work. The
    // user-facing gesture is an input event, and the assertions below still
    // prove both the persisted preference and the accessible projection.
    const before = await chrome.evaluate(() => {
      const button = document.querySelector("[data-collab-action='presence-announcements']");
      const announcement = document.querySelector("[data-collab-presence-announcement]");
      if (!button || !announcement) return null;
      return {
        pressed: button.getAttribute("aria-pressed"),
        role: announcement.getAttribute("role"),
        live: announcement.getAttribute("aria-live"),
      };
    });
    assert.deepEqual(before, { pressed: "true", role: "status", live: "polite" });
    await chrome.click("[data-collab-action='presence-announcements']");
    const preference = await pollUntil(
      chrome,
      "presence-announcement opt-out persists and projects as pressed=false",
      () => {
        const button = document.querySelector("[data-collab-action='presence-announcements']");
        return button?.getAttribute("aria-pressed") === "false" &&
          localStorage.getItem("opendoc.presence-announcements") === "off"
          ? {
              before: "true",
              after: button.getAttribute("aria-pressed"),
              stored: localStorage.getItem("opendoc.presence-announcements"),
              role: document.querySelector("[data-collab-presence-announcement]")?.getAttribute("role"),
              live: document.querySelector("[data-collab-presence-announcement]")?.getAttribute("aria-live"),
            }
          : null;
      },
      null,
    );
    assert.deepEqual(preference, {
      before: "true",
      after: "false",
      stored: "off",
      role: "status",
      live: "polite",
    });

    await bobChrome.evaluate(() => window.__OPENDOC_COLLAB__.disconnect());
    await pollUntil(
      chrome,
      "muted bob departure reaches alice without an announcement",
      () => {
        const bob = document.querySelector('[data-collab-peer="actor-bob"]');
        const announcement = document.querySelector("[data-collab-presence-announcement]");
        return !bob && announcement?.textContent === "" ? true : null;
      },
      null,
    );

    const bob = await rejoinSessionAfterLeaving(bobChrome, {
      serviceUrl: collabService.url,
      subject: "bob",
      apiKey: BOB_KEY,
      documentUuid,
      displayName: "Bob Brown",
    });
    assert.ok(!bob?.error, `bob could not reconnect: ${JSON.stringify(bob)}`);
    await waitForPhase(bobChrome, "live", "bob rejoins after the preference test");
    await pollUntil(
      chrome,
      "alice sees bob after he rejoins",
      () => (document.querySelector('[data-collab-peer="actor-bob"]') ? true : null),
      null,
    );
  });

  await check("a dropped socket reconnects and resubmits the work it was holding", async () => {
    // The socket goes; the intention to be connected stays. Everything that
    // runs from here is the shipped reconnect path.
    await chrome.evaluate(() => window.__OPENDOC_COLLAB__.dropSocket());
    const dropped = await pollUntil(
      chrome,
      "alice's pill reports the drop",
      () => {
        const status = window.__OPENDOC_COLLAB__.status();
        return status.phase === "reconnecting" || status.phase === "closed"
          ? { phase: status.phase, notice: status.notice }
          : null;
      },
      null,
      { tries: 100, delayMs: 50 },
    );
    assert.equal(dropped.phase, "reconnecting");
    assert.ok(dropped.notice, "a dropped socket must say so");

    // Work authored while disconnected, on both sides. Alice's cannot be sent
    // yet; Bob's is committed and will be in the log alice's reconnect reads.
    await typeIntoDocument(chrome, "offline-alice");
    await typeIntoDocument(bobChrome, "online-bob");
    await waitForPhase(chrome, "live", "alice reconnects");

    // The welcome is the resynchronisation: alice's replica adopts the
    // service's base and log, then replays the tail the service never got.
    await waitForBodyText(chrome, "online-bob", "alice catches up on what she missed");
    await waitForBodyText(bobChrome, "offline-alice", "alice's offline work reaches bob");
    const aliceText = await bodyText(chrome);
    const bobText = await bodyText(bobChrome);
    assert.equal(aliceText, bobText, `the replicas diverged across a reconnect:\n${aliceText}\n${bobText}`);
    const notice = await chrome.evaluate(() => window.__OPENDOC_COLLAB__.status().notice);
    assert.equal(notice.kind, "resynchronised", JSON.stringify(notice));
  });

  await check("a service that stops answering is reported as over, not as reconnecting for ever", async () => {
    await collabService.stop();
    await chrome.evaluate(() => window.__OPENDOC_COLLAB__.dropSocket());
    // A browser is never told why a socket handshake failed, so the client
    // cannot distinguish "down" from "revoked" — it retries a bounded number
    // of times and then says the session is over rather than lying about it.
    const finished = await pollUntil(
      chrome,
      "alice's session is declared over",
      () => {
        const status = window.__OPENDOC_COLLAB__.status();
        return status.phase === "closed" ? { notice: status.notice } : null;
      },
      null,
      { tries: 600, delayMs: 150 },
    );
    assert.ok(finished.notice, "the end of a session must be explained");
    assert.equal(finished.notice.resumable, false);
    assert.ok(
      finished.notice.message.includes("attempts"),
      `the message must say it gave up trying: ${finished.notice.message}`,
    );
    const label = await chrome.evaluate(
      () => document.querySelector("[data-collab-phase]")?.textContent ?? "",
    );
    assert.equal(label, "Disconnected");
    // Bob's replica keeps its document: losing the service is not losing work.
    assert.ok((await bodyText(bobChrome)).includes("offline-alice"));
  });

  await check("a burst of typing is chunked to the cap the service announced", async () => {
    // P1-8, end to end through the shipped path. The cap is the service's own
    // number and travels in the welcome, so this deployment sets it to one and
    // every tick that finds more than one unsent operation has to chunk.
    // Before, such a tick built one oversized batch, the service refused it,
    // and nothing ever cleared the refusal: the pill stayed on "Live" while
    // every further keystroke stayed local for ever.
    const origin = new URL(server.url).origin;
    await collabService.close();
    collabService = await startCollaborationService(origin, {
      OPENDOC_SERVICE_MAX_OPERATIONS_PER_SUBMIT: "1",
    });
    await openBlankDocument(chrome);
    const alice = await joinSession(chrome, {
      serviceUrl: collabService.url,
      subject: "alice",
      apiKey: ALICE_KEY,
      documentUuid: "",
      displayName: "Alice Anderson",
    });
    assert.ok(!alice?.error, `alice could not connect: ${JSON.stringify(alice)}`);
    await waitForPhase(chrome, "live", "alice's session goes live against the capped service");

    // Faster than the 250 ms pump can drain at one operation per frame, so a
    // backlog is certain and every tick after the first carries several.
    const burst = "chunk-me-0123456789";
    await typeIntoDocument(chrome, burst);

    const settled = await pollUntil(
      chrome,
      "every keystroke of the burst to be made durable",
      (want) => {
        const status = window.__OPENDOC_COLLAB__.status();
        return status.phase === "live" && status.pending_operations === 0 && status.acknowledged_seq >= want
          ? { acknowledged: status.acknowledged_seq, notice: status.notice, canSubmit: status.can_submit }
          : null;
      },
      burst.length,
      { tries: 300, delayMs: 150 },
    );
    assert.ok(settled.canSubmit, `the session must still be able to send: ${JSON.stringify(settled)}`);
    assert.ok(
      !settled.notice || !settled.notice.kind.startsWith("refused-"),
      `nothing may be refused when the outbox chunks to the service's cap: ${JSON.stringify(settled.notice)}`,
    );
    const text = await bodyText(chrome);
    assert.ok(text.includes(burst), `every character must have reached the document: ${JSON.stringify(text)}`);
  });


  // ---- The widened agreement fixture (PLAN88 §7) --------------------------
  //
  // "Rust's page assignment matches where Chrome drew each block" above is the
  // check ADR 0014 calls decisive, and it was built out of plain paragraphs,
  // one heading and a bulleted list. That is why three measured divergences
  // shipped under a green gate: a size mark took its leading from the block
  // instead of from the run (22px where Chrome draws 36), a monospace run made
  // Chrome's line box a pixel taller than Rust's, and a list run that changed
  // marker inherited a wrapper margin-bottom the flow never counted. None of
  // them could be seen through a fixture that contains none of those things.
  //
  // So the fixture below carries **every inline kind and every block kind**:
  // code, size, superscript, subscript, underline, strike and colour marks, a
  // link, a mention, a citation, a footnote reference, a heading, bulleted,
  // ordered and checklist runs with a marker change inside one run, an
  // explicit page break and a table. It asserts the fixture actually contains
  // them before it asserts anything about pagination, because a fixture that
  // silently failed to build would make the check vacuous — which is the
  // failure mode §7 is about.

  /** Puts the caret at the end of the last run of the last block. */
  async function caretAtEndOfDocument() {
    await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      host.focus();
      const runs = host.querySelectorAll("[data-inline-id]");
      const run = runs[runs.length - 1] ?? host;
      const range = document.createRange();
      range.selectNodeContents(run);
      range.collapse(false);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    });
    await settle(chrome);
  }

  /** Selects the whole content of the block whose text starts with `prefix`. */
  async function selectBlockStartingWith(prefix) {
    const found = await chrome.evaluate((want) => {
      const host = document.querySelector('[contenteditable="true"]');
      const block = [...host.querySelectorAll("[data-block-id]")].find((candidate) =>
        (candidate.textContent ?? "").startsWith(want),
      );
      if (!block) return false;
      const range = document.createRange();
      range.selectNodeContents(block);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      host.focus();
      return true;
    }, prefix);
    assert.ok(found, `no block starting with ${JSON.stringify(prefix)} to select`);
    await settle(chrome);
  }

  /** Types one paragraph and leaves the caret on a fresh block after it. */
  async function typeParagraph(text) {
    await caretAtEndOfDocument();
    await chrome.type(text);
    await chrome.press("Enter");
    await settle(chrome);
  }

  /**
   * Every block's rendered box against the content box of the page Rust
   * assigned it, in both directions — nothing may overflow its page, and the
   * block that opens a page must not have fitted on the page before it.
   *
   * The same measurement the plain-paragraph check above makes; stated here as
   * a function so the widened fixture asserts exactly the same thing about a
   * much larger document rather than something weaker.
   */
  const measureAgreement = () =>
    chrome.evaluate(() => {
      const sheets = [...document.querySelectorAll(".page-sheet")].map((sheet) =>
        sheet.getBoundingClientRect(),
      );
      const blocks = [
        ...document.querySelectorAll('[contenteditable="true"] [data-block-id][data-page-index]'),
      ];
      const flow = document.querySelector("[data-page]");
      if (sheets.length < 2 || blocks.length === 0 || !flow) return null;
      const style = window.getComputedStyle(flow);
      const padTop = parseFloat(style.paddingTop);
      const padBottom = parseFloat(style.paddingBottom);
      const disagreements = [];
      for (const block of blocks) {
        const page = Number(block.dataset.pageIndex);
        const sheet = sheets[page];
        const rect = block.getBoundingClientRect();
        if (!sheet) {
          disagreements.push({ id: block.dataset.blockId, page, reason: "no such page drawn" });
          continue;
        }
        const top = sheet.top + padTop;
        const bottom = sheet.bottom - padBottom;
        if (rect.top < top - 0.5) {
          disagreements.push({
            id: block.dataset.blockId,
            page,
            reason: `drawn ${(top - rect.top).toFixed(2)}px above page ${page}'s content box`,
          });
        } else if (rect.bottom > bottom + 0.5 && rect.height <= bottom - top) {
          disagreements.push({
            id: block.dataset.blockId,
            page,
            reason: `drawn ${(rect.bottom - bottom).toFixed(2)}px below page ${page}'s content box`,
          });
        }
      }
      for (let page = 1; page < sheets.length; page += 1) {
        const onPage = blocks.filter((block) => Number(block.dataset.pageIndex) === page);
        const onPrevious = blocks.filter((block) => Number(block.dataset.pageIndex) === page - 1);
        if (onPage.length === 0 || onPrevious.length === 0) continue;
        const opener = onPage[0];
        if (opener.previousElementSibling?.classList.contains("doc-page-break")) continue;
        // A block that *is* a page break, or that follows one, is placed by
        // instruction rather than by overflow.
        if (opener.classList.contains("doc-page-break")) continue;
        const previous = onPrevious[onPrevious.length - 1];
        const last = previous.getBoundingClientRect();
        const height = opener.getBoundingClientRect().height;
        const room = sheets[page - 1].bottom - padBottom - last.bottom;
        // The block would also have had to carry the margin between it and
        // the block above. Pagination overwrites the opener's own `margin-top`
        // to place it, so the readable half of that collapse is the previous
        // block's `margin-bottom` — which is the half that actually decides it
        // for a paragraph, a list wrapper or a table.
        const gap = parseFloat(window.getComputedStyle(previous).marginBottom) || 0;
        if (height + gap <= room - 0.5) {
          disagreements.push({
            id: opener.dataset.blockId,
            page,
            reason: `opened page ${page} although ${(height + gap).toFixed(2)}px fitted in the ${room.toFixed(2)}px left on page ${page - 1}`,
          });
        }
      }
      return { pages: sheets.length, blocks: blocks.length, disagreements };
    });

  /**
   * Builds the widened fixture on a 3-inch page and returns what it contains.
   *
   * Structure first, marks second: applying a mark needs a selection, and a
   * caret that has just carried a mark forward would put it on text the next
   * step did not mean to mark. Everything plain is typed first and the marks
   * are then applied to named blocks.
   */
  async function buildWidenedFixture() {
    // A fresh page rather than `openBlankDocument` alone: the collaboration
    // checks above leave a live session attached, and a reload drops it.
    await chrome.goto(server.url);
    await chrome.waitUntil(
      "the app after a reload",
      () => !!document.querySelector('[data-action="new-document"], [contenteditable="true"]'),
      { tries: 600, delayMs: 250 },
    );
    // A reload with unsaved work raises the crash-recovery offer (ADR 0005),
    // and it sits over the home screen until it is answered.
    await clearRecoveryOffers();
    for (let attempt = 0; attempt < 10; attempt += 1) {
      if (!(await dismissModal(chrome))) break;
      await settle(chrome);
    }
    await openBlankDocument(chrome);
    await menuAction("File", "page-setup");
    await poll("the page setup prompt", () =>
      answerPrompt("Page setup", {
        size: "custom",
        orientation: "landscape",
        width: "8.50",
        height: "3.00",
        top: "0.50",
        bottom: "0.50",
        start: "0.50",
        end: "0.50",
      }),
    );
    await poll("the short page", async () => {
      const measured = await sheetGeometry();
      return measured && measured.height === 288 ? measured : null;
    });

    // A reference to cite, added through the Citations panel the way a user
    // would.
    await menuAction("View", "toggle-panel:citations");
    await settle(chrome);
    await chrome.click('[data-action="add-reference"]');
    await answerNamedPrompt("Add reference", {
      title: "On Measured Divergence",
      authors: "A. Auditor",
      issued: "2026",
      doi: "",
      url: "",
    });
    await settle(chrome);

    await chrome.type("The quick brown fox jumps over the lazy dog, and then keeps running until the line has to wrap.");
    await chrome.press("Enter");
    await settle(chrome);
    await typeParagraph("A heading follows this one.");
    await chrome.type("Heading over the fixture");
    await settle(chrome);
    await menuAction("Format", "style:heading:1");
    await settle(chrome);
    await caretAtEndOfDocument();
    await chrome.press("Enter");
    await menuAction("Format", "style:paragraph");
    await settle(chrome);

    // One paragraph per inline kind, named by its first word so the marks can
    // find it again.
    await typeParagraph("codeblock a run set in the monospace face which is a taller line box than the sans one beside it");
    await typeParagraph("sizeblock a run set at eighteen points inside an eleven point paragraph");
    await typeParagraph("scriptblock a run raised off the baseline and one dropped below it");
    await typeParagraph("decoratedblock underlined struck and coloured all at once");

    await caretAtEndOfDocument();
    await chrome.type("mentionblock ");
    await settle(chrome);
    await menuAction("Insert", "insert-mention");
    await answerNamedPrompt("Mention", { label: "@alice.anderson" });
    await settle(chrome);
    await caretAtEndOfDocument();
    await chrome.press("Enter");
    await settle(chrome);

    await chrome.type("citeblock ");
    await settle(chrome);
    await menuAction("Insert", "insert-citation");
    await answerNamedPrompt("Insert citation", { locator: "" });
    await settle(chrome);
    await caretAtEndOfDocument();
    await chrome.press("Enter");
    await settle(chrome);

    await chrome.type("noteblock ");
    await settle(chrome);
    await menuAction("Insert", "insert-footnote");
    await settle(chrome);
    await caretAtEndOfDocument();
    await chrome.press("Enter");
    await settle(chrome);

    await chrome.type("linkblock ");
    await settle(chrome);
    await menuAction("Insert", "insert-link");
    await answerNamedPrompt("Insert link", {
      text: "a hyperlink in the flow",
      href: "https://example.invalid/target",
    });
    await settle(chrome);
    await caretAtEndOfDocument();
    await chrome.press("Enter");
    await settle(chrome);

    // A list run whose marker changes partway: `ListWriter` closes the
    // outermost `.doc-list` and opens another, and the closed wrapper's
    // margin-bottom is 13.33px of real space.
    await chrome.type("bullet one of the list");
    await settle(chrome);
    await menuAction("Format", "style:list:bullet");
    await settle(chrome);
    await caretAtEndOfDocument();
    await chrome.press("Enter");
    await chrome.type("bullet two of the list");
    await settle(chrome);
    await caretAtEndOfDocument();
    await chrome.press("Enter");
    await chrome.type("ordered one after the marker changes");
    await settle(chrome);
    await menuAction("Format", "style:list:ordered");
    await settle(chrome);
    await caretAtEndOfDocument();
    await chrome.press("Enter");
    await chrome.type("ordered two after the marker changes");
    await settle(chrome);
    await caretAtEndOfDocument();
    await chrome.press("Enter");
    await chrome.type("checklist item with a box for a marker");
    await settle(chrome);
    await menuAction("Format", "style:list:checklist");
    await settle(chrome);
    await caretAtEndOfDocument();
    await chrome.press("Enter");
    await menuAction("Format", "style:paragraph");
    await settle(chrome);

    await chrome.type("after the lists, a paragraph long enough to wrap at least once on this page");
    await chrome.press("Enter");
    await settle(chrome);
    await menuAction("Insert", "insert-page-break");
    await settle(chrome);

    for (let i = 0; i < 6; i += 1) {
      await typeParagraph(`tail paragraph ${i} with a little more text on it so that it is not trivially short`);
    }

    // Marks, applied to the blocks that were typed for them.
    await selectBlockStartingWith("codeblock");
    // Code and script marks live in the Format menu only; the toolbar has no
    // button for either, and clicking the menu item while the menu is shut
    // lands on whatever is drawn over it.
    await menuAction("Format", "mark:code");
    await settle(chrome);
    await selectBlockStartingWith("sizeblock");
    await chrome.evaluate(() => {
      const select = document.querySelector('select[data-select="size"]');
      select.value = "18";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await settle(chrome);
    await selectBlockStartingWith("scriptblock");
    await menuAction("Format", "mark:superscript");
    await settle(chrome);
    await selectBlockStartingWith("decoratedblock");
    await chrome.click('.tb[data-action="mark:underline"]');
    await settle(chrome);
    await selectBlockStartingWith("decoratedblock");
    await chrome.click('.tb[data-action="mark:strike"]');
    await settle(chrome);
    await selectBlockStartingWith("decoratedblock");
    await chrome.evaluate(() => {
      const input = document.querySelector('input[data-color="color"]');
      input.value = "#cc0000";
      input.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await settle(chrome);

    // The table goes in last: it is the only block whose own runs would
    // otherwise become "the end of the document" for the caret helper.
    await caretAtEndOfDocument();
    await menuAction("Insert", "insert-table");
    await answerNamedPrompt("Insert table", { rows: "2", columns: "3" });
    await chrome.waitUntil(
      "the fixture's table",
      () => !!document.querySelector('[contenteditable="true"] table[data-block-id] td'),
      { tries: 120, delayMs: 100 },
    );
    await settle(chrome);

    return chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const count = (selector) => host.querySelectorAll(selector).length;
      return {
        code: count(".mark-code"),
        size: count(".mark-size"),
        superscript: count(".mark-superscript"),
        underline: count(".mark-underline"),
        strike: count(".mark-strike"),
        colour: count(".mark-color"),
        link: count("a.run-link"),
        mention: count(".mention"),
        citation: count(".citation-label"),
        footnote: count(".footnote-ref"),
        heading: count("h1[data-block-id]"),
        bulletList: count("ul.doc-list:not(.doc-checklist)"),
        orderedList: count("ol.doc-list"),
        checklist: count("ul.doc-checklist"),
        pageBreak: count(".doc-page-break"),
        table: count("table[data-block-id]"),
        blocks: count("[data-block-id]"),
      };
    });
  }

  let widenedFixture = null;

  await check("the widened fixture actually carries every inline and block kind", async () => {
    // This is the guard on the guard. The agreement check below is only worth
    // anything if the document it measures contains the things that broke it,
    // and a fixture built by driving a real UI can quietly fail to build one
    // of them. So the composition is asserted, by name, before any geometry
    // is.
    widenedFixture = await buildWidenedFixture();
    const required = [
      "code",
      "size",
      "superscript",
      "underline",
      "strike",
      "colour",
      "link",
      "mention",
      "citation",
      "footnote",
      "heading",
      "bulletList",
      "orderedList",
      "checklist",
      "pageBreak",
      "table",
    ];
    const missing = required.filter((kind) => !widenedFixture[kind]);
    assert.deepEqual(
      missing,
      [],
      `the fixture is missing ${missing.join(", ")}: ${JSON.stringify(widenedFixture)}`,
    );
    // Two list wrappers, not one: the run changes marker partway, which is
    // what closes the first `.doc-list` and opens the next.
    assert.ok(
      widenedFixture.bulletList >= 1 && widenedFixture.orderedList >= 1,
      `the list run did not change marker: ${JSON.stringify(widenedFixture)}`,
    );
  });

  await check("no document run is drawn wider than the advances Rust summed", async () => {
    // ADR 0014's guarantee, read back out of Chrome: a run's width *is* the
    // sum of its glyphs' advances. `.mark-code`, `.citation-label` and
    // `.mention` each used to carry horizontal padding the layout never
    // measured — 2px, 2px and 6px — which made a paragraph of six short code
    // runs one line in Rust and two in Chrome. The padding is gone from the
    // stylesheet rather than modelled, because it is a skin and a skin must
    // not decide where the pages break.
    const padded = await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const offenders = [];
      for (const selector of [".mark-code", ".citation-label", ".mention", ".footnote-ref", ".run-link"]) {
        for (const element of host.querySelectorAll(selector)) {
          const style = window.getComputedStyle(element);
          const left = parseFloat(style.paddingLeft);
          const right = parseFloat(style.paddingRight);
          const start = parseFloat(style.borderLeftWidth);
          const end = parseFloat(style.borderRightWidth);
          const margin = parseFloat(style.marginLeft) + parseFloat(style.marginRight);
          if (left || right || start || end || margin) {
            offenders.push({ selector, left, right, start, end, margin });
          }
        }
      }
      return offenders;
    });
    assert.deepEqual(
      padded,
      [],
      `these runs occupy width the layout engine does not measure: ${JSON.stringify(padded)}`,
    );
  });

  await check("Chrome draws the line boxes the layout engine measures with", async () => {
    // The *other half* of the loop, like the projected-type-scale check above:
    // this is a measurement of Chrome, not of Rust. A run marked 18pt inside
    // an 11pt paragraph makes its line box 36px, because a unitless
    // `line-height: 1.5` multiplies the span's own size; a monospace run makes
    // it 23px, because Blink rounds each face's ascent and descent to whole
    // pixels and takes the union. Those three numbers are hard-coded in
    // `opendoc-layout`'s own tests as the answers it must produce, so if
    // Chrome — or this stylesheet — ever stopped producing them, those tests
    // would be pinning the wrong thing and nothing else would say so. Whether
    // *Rust* agrees is the agreement check below; the layout used to compute
    // 22px here and call the block exactly measured.
    const boxes = await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const find = (prefix) =>
        [...host.querySelectorAll("p[data-block-id]")].find((block) =>
          (block.textContent ?? "").startsWith(prefix),
        );
      const measure = (prefix) => {
        const block = find(prefix);
        if (!block) return null;
        const rect = block.getBoundingClientRect();
        // A block container has exactly one client rect, so the line count
        // comes from a range over its contents: one distinct top per line
        // fragment.
        const range = document.createRange();
        range.selectNodeContents(block);
        const tops = new Set(
          [...range.getClientRects()].map((line) => Math.round(line.top)),
        );
        return { height: rect.height, lines: tops.size };
      };
      return {
        plain: measure("after the lists"),
        sized: measure("sizeblock"),
        code: measure("codeblock"),
      };
    });
    assert.ok(boxes.sized && boxes.code && boxes.plain, JSON.stringify(boxes));
    // Every line of the 18pt paragraph is 36px, and every line of the plain
    // one is 22px, so the ratio is the thing to assert rather than a height
    // that depends on how many times the text happened to wrap.
    const perLine = (box) => box.height / box.lines;
    assert.ok(
      Math.abs(perLine(boxes.sized) - 36) < 0.6,
      `an 18pt run's line box is ${perLine(boxes.sized).toFixed(2)}px, not the 36px Chrome draws`,
    );
    assert.ok(
      Math.abs(perLine(boxes.plain) - 22) < 0.6,
      `a plain line box is ${perLine(boxes.plain).toFixed(2)}px`,
    );
    assert.ok(
      Math.abs(perLine(boxes.code) - 23) < 0.6,
      `a monospace run's line box is ${perLine(boxes.code).toFixed(2)}px, not the 23px Chrome draws`,
    );
  });

  await check("Rust's page assignment matches Chrome over every inline and block kind", async () => {
    // The check ADR 0014 calls decisive, over the widened fixture. Any
    // line-count or line-height disagreement anywhere in the flow accumulates
    // and pushes a later block out of the page Rust named, so this is
    // sensitive to exactly the mismatches that shipped: before the fixes, the
    // 18pt paragraph alone put eight of forty-one blocks outside their pages.
    const agreement = await poll("the widened fixture, paginated", () => measureAgreement());
    assert.ok(agreement.pages >= 3, `expected several pages, got ${agreement.pages}`);
    assert.ok(
      agreement.blocks >= 20,
      `only ${agreement.blocks} blocks carried a page assignment`,
    );
    assert.deepEqual(
      agreement.disagreements,
      [],
      `Rust's pagination disagrees with what Chrome drew: ${JSON.stringify(agreement.disagreements)}`,
    );
  });

  await check("the PDF of the widened fixture carries what the screen shows", async () => {
    // ADR 0016's claim is that the PDF agrees with the screen *and* says what
    // it could not carry. Colour, highlight, underline, strike and links used
    // to be dropped with no warning at all, and every footnote reference
    // printed a literal `0`.
    const geometry = await sheetGeometry();
    const download = await captureDownload("export-pdf");
    assert.ok(download.body.startsWith("%PDF-"), "the payload is not a PDF");
    const media = /\/MediaBox \[0 0 ([0-9.]+) ([0-9.]+)\]/.exec(download.body);
    assert.ok(media, "the PDF states no MediaBox");
    assert.equal(Math.round(Number(media[2])), Math.round(geometry.height * 0.75));
    const pages = download.body.split("/Type /Page\n").length - 1;
    // The body pages must be the pages Chrome drew. The footnote bodies are
    // the one thing the PDF places differently — the editing surface keeps
    // them in an area under the whole page stack, which is not a page — so
    // they may add one page at the end, and the export warns by name that it
    // did (`pdf-footnotes-after-the-body`).
    assert.ok(
      pages >= geometry.sheets && pages <= geometry.sheets + 1,
      `the PDF has ${pages} page(s) and Chrome drew ${geometry.sheets} sheet(s)`,
    );
    assert.ok(
      download.body.includes("/Subtype /Link") && download.body.includes("example.invalid/target"),
      "the link was dropped from the PDF",
    );
    assert.ok(download.body.includes("0.8 0 0 rg"), "the colour mark was dropped from the PDF");
    assert.ok(
      download.body.includes("] 0 d\n"),
      "the page-break rule is dashed on screen and solid on paper",
    );
  });

  // ---- Tab, paste and direction (PLAN88 P2: the editor items) ------------
  //
  // Every one of these is a Chrome behaviour rather than a model fact, and
  // the first three were measured before they were implemented: a Tab this
  // handler does not consume moves focus out of the editable body and
  // inserts nothing, which is what makes "let the key through" a working
  // escape hatch rather than a hope.

  /** Puts the caret inside the target's first run, focused and typeable. */
  const caretInto = (browser, selector, atEnd = false) =>
    browser.evaluate(
      (target, collapseToEnd) => {
        const host = document.querySelector('[contenteditable="true"]');
        const element = target ? document.querySelector(target) : host;
        if (!element) return false;
        const run = element.querySelector("[data-inline-id]") ?? element;
        const range = document.createRange();
        range.selectNodeContents(run);
        range.collapse(!collapseToEnd);
        const selection = window.getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
        host.focus();
        return true;
      },
      selector,
      atEnd,
    );

  const activeAction = (browser) =>
    browser.evaluate(() => {
      const active = document.activeElement;
      if (!active) return "none";
      return `${active.tagName}:${active.dataset?.action ?? active.className ?? ""}`;
    });

  /** The ids of the cells the grid draws, in reading order. */
  const drawnCellIds = () =>
    chrome.evaluate(() =>
      [...document.querySelectorAll('[contenteditable="true"] td[data-cell-id]')].map(
        (cell) => cell.dataset.cellId,
      ),
    );

  await check("Tab leaves the document instead of typing a tab", async () => {
    await openBlankDocument(chrome);
    await chrome.type("hello");
    await settle(chrome);
    await chrome.press("Tab");
    await settle(chrome);
    const body = await chrome.evaluate(
      () => document.querySelector('[contenteditable="true"]').innerText,
    );
    assert.equal(body, "hello", `Tab typed into the document: ${JSON.stringify(body)}`);
    const active = await activeAction(chrome);
    assert.ok(
      !active.startsWith("DIV"),
      `focus stayed inside the document after Tab: ${active}`,
    );
  });

  await check("Tab steps between table cells and adds a row after the last one", async () => {
    await insertTable(2, 3);
    const cells = await drawnCellIds();
    assert.ok(cells.length >= 4, `expected a grid, got ${cells.length} cell(s)`);
    const caretCell = () =>
      chrome.evaluate(() => {
        const node = window.getSelection()?.focusNode;
        const element = node instanceof Element ? node : node?.parentElement;
        return element?.closest("td[data-cell-id]")?.dataset.cellId ?? "none";
      });

    assert.ok(await caretInto(chrome, `td[data-cell-id="${cells[0]}"]`), "no first cell");
    await settle(chrome);
    await chrome.press("Tab");
    await settle(chrome);
    assert.equal(await caretCell(), cells[1], "Tab did not reach the next cell");
    await chrome.press("Tab", { modifiers: 8 });
    await settle(chrome);
    assert.equal(await caretCell(), cells[0], "Shift+Tab did not reach the previous cell");

    // Google Docs grows a table when Tab leaves its final cell. The appended
    // row has the same number of cells and the caret starts in its first cell.
    await caretInto(chrome, `td[data-cell-id="${cells[cells.length - 1]}"]`);
    await settle(chrome);
    await chrome.press("Tab");
    await settle(chrome);
    const grownCells = await drawnCellIds();
    assert.equal(
      grownCells.length,
      cells.length + 3,
      `Tab from the last cell did not append a full row: ${grownCells.length} cells`,
    );
    assert.equal(await caretCell(), grownCells[cells.length], "Tab did not reach the new row's first cell");
  });

  await check("typing in an empty table cell resolves its cell-local paragraph", async () => {
    await insertTable(1, 1);
    await caretInCell(0, 0);
    await chrome.type("cell-local text");
    const text = await chrome.waitUntil(
      "text in the empty cell",
      () => {
        const cell = document.querySelector('[contenteditable="true"] td[data-cell-id]');
        return cell?.textContent?.trim() === "cell-local text" ? cell.textContent.trim() : null;
      },
      { tries: 60, delayMs: 100 },
    );
    assert.equal(text, "cell-local text");
    const bodyLead = await chrome.evaluate(
      () => document.querySelector('[contenteditable="true"] > p[data-block-id]')?.textContent?.trim(),
    );
    assert.equal(bodyLead, "before the table", "empty-cell typing must not edit the body or another cell");
  });

  await check("table row and column bands create whole-grid selections", async () => {
    await insertTable(2, 3);
    const selected = await chrome.evaluate(() => {
      const table = document.querySelector('[contenteditable="true"] table[data-block-id]');
      const cells = table.querySelectorAll("td[data-cell-id]");
      const cell = cells[1];
      const rect = cell.getBoundingClientRect();
      const tableRect = table.getBoundingClientRect();
      const describe = () => {
        const selection = window.getSelection();
        const cellFor = (node) => (node instanceof Element ? node : node?.parentElement)?.closest("td[data-cell-id]");
        return {
          anchor: cellFor(selection.anchorNode)?.dataset.cellId ?? "none",
          focus: cellFor(selection.focusNode)?.dataset.cellId ?? "none",
        };
      };
      table.dispatchEvent(new MouseEvent("mousedown", {
        bubbles: true,
        button: 0,
        clientX: rect.left + rect.width / 2,
        clientY: tableRect.top + 5,
      }));
      const column = describe();
      const rowCell = cells[3];
      const rowRect = rowCell.getBoundingClientRect();
      table.dispatchEvent(new MouseEvent("mousedown", {
        bubbles: true,
        button: 0,
        clientX: tableRect.left + 5,
        clientY: rowRect.top + rowRect.height / 2,
      }));
      return { column, row: describe(), ids: Array.from(cells, (node) => node.dataset.cellId) };
    });
    assert.equal(selected.column.anchor, selected.ids[1], "column band did not start at its top cell");
    assert.equal(selected.column.focus, selected.ids[4], "column band did not reach its bottom cell");
    assert.equal(selected.row.anchor, selected.ids[3], "row band did not start at its first cell");
    assert.equal(selected.row.focus, selected.ids[5], "row band did not reach its last cell");
  });

  await check("Escape releases the editor for a keyboard-only user", async () => {
    await openBlankDocument(chrome);
    await chrome.type("list");
    await settle(chrome);
    assert.ok(
      await chrome.evaluate(
        () => document.activeElement === document.querySelector('[contenteditable="true"]'),
      ),
      "the editor was not focused to begin with",
    );
    await chrome.press("Escape");
    await settle(chrome);
    assert.ok(
      await chrome.evaluate(
        () => document.activeElement !== document.querySelector('[contenteditable="true"]'),
      ),
      "Escape did not release the editor, so a list is still a trap",
    );
  });

  await check("a paste keeps the formatting the clipboard carried", async () => {
    // `EditorInput.html` was populated by the frontend and read by nothing,
    // so every paste arrived as unformatted text. There is no CDP clipboard
    // primitive, so the paste is synthesised the way the drop check does it.
    await openBlankDocument(chrome);
    await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const transfer = new DataTransfer();
      transfer.setData("text/plain", "plain bold");
      transfer.setData("text/html", "<p>plain <b>bold</b></p>");
      host.dispatchEvent(
        new ClipboardEvent("paste", {
          clipboardData: transfer,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    await waitForBodyText(chrome, "plain bold");
    const weight = await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const marked = [...host.querySelectorAll("[data-inline-id]")].find(
        (run) => run.textContent === "bold",
      );
      return marked ? window.getComputedStyle(marked).fontWeight : null;
    });
    assert.ok(weight, "no run carrying the pasted bold text");
    assert.ok(
      Number(weight) >= 600 || weight === "bold" || weight === "bolder",
      `the pasted bold arrived with computed font-weight ${weight}`,
    );
  });

  await check("a hostile paste arrives as words, not as markup", async () => {
    await openBlankDocument(chrome);
    await chrome.evaluate(() => {
      window.__pasteProbe = false;
      const host = document.querySelector('[contenteditable="true"]');
      const transfer = new DataTransfer();
      transfer.setData("text/plain", "safe");
      transfer.setData(
        "text/html",
        '<script>window.__pasteProbe = true;<\/script><img src=x onerror="window.__pasteProbe = true">safe',
      );
      host.dispatchEvent(
        new ClipboardEvent("paste", {
          clipboardData: transfer,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    await waitForBodyText(chrome, "safe");
    assert.equal(
      await chrome.evaluate(() => window.__pasteProbe === true),
      false,
      "script from the clipboard ran in the page",
    );
    assert.equal(
      await chrome.evaluate(
        () => document.querySelectorAll('[contenteditable="true"] img, [contenteditable="true"] script').length,
      ),
      0,
      "markup from the clipboard reached the document",
    );
  });

  await check("a right-to-left paragraph reaches computed direction", async () => {
    // The command, the block property and the CSS were all finished in Rust
    // and no control could reach any of them.
    await openBlankDocument(chrome);
    await chrome.type("shalom");
    await settle(chrome);
    await chrome.click('.tb[data-action="direction:rtl"]');
    await settle(chrome);
    const direction = await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const block = host.querySelector("[data-block-id]");
      return block ? window.getComputedStyle(block).direction : null;
    });
    assert.equal(direction, "rtl", `the paragraph computed to direction ${direction}`);
    await chrome.click('.tb[data-action="direction:ltr"]');
    await settle(chrome);
    assert.equal(
      await chrome.evaluate(() => {
        const host = document.querySelector('[contenteditable="true"]');
        const block = host.querySelector("[data-block-id]");
        return block ? window.getComputedStyle(block).direction : null;
      }),
      "ltr",
      "the paragraph did not come back left-to-right",
    );
  });

  await check("deleting across a table boundary is not silently ignored", async () => {
    // `apply_editor_input` refused any selection whose ends were in different
    // containers, and did it by reporting success.
    await insertTable(2, 3);
    const cells = await drawnCellIds();
    await caretInto(chrome, `td[data-cell-id="${cells[0]}"]`);
    await settle(chrome);
    await chrome.type("inside");
    await settle(chrome);

    await chrome.evaluate((cellId) => {
      const host = document.querySelector('[contenteditable="true"]');
      const first = host.querySelector("[data-inline-id]");
      const cell = document.querySelector(`td[data-cell-id="${cellId}"]`);
      const target = cell.querySelector("[data-inline-id]");
      const range = document.createRange();
      range.setStart(first.firstChild ?? first, 3);
      range.setEnd(target.firstChild ?? target, 3);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      host.focus();
    }, cells[0]);
    await settle(chrome);
    await chrome.press("Backspace");
    await settle(chrome);

    const after = await chrome.evaluate((cellId) => ({
      lead: document.querySelector('[contenteditable="true"] [data-block-id]')?.innerText ?? "",
      cell: document.querySelector(`td[data-cell-id="${cellId}"]`)?.innerText ?? "",
      rows: document.querySelectorAll('[contenteditable="true"] tr[data-row-id]').length,
    }), cells[0]);
    assert.equal(
      after.lead,
      "bef",
      `the paragraph kept text the selection covered: ${after.lead}`,
    );
    assert.ok(
      after.cell.trim() === "ide",
      `the cell kept the head the selection covered: ${JSON.stringify(after.cell)}`,
    );
    assert.equal(after.rows, 2, `a text selection changed the table's rows: ${after.rows}`);
  });

  // ---- Two tabs over one browser store ------------------------------------
  //
  // Two tabs on one origin are two Rust runtimes over one IndexedDB database.
  // Each hydrates its own `MirroredVolume` once at boot and never sees the
  // other's writes again, so without arbitration both flush divergent views of
  // the same keys. `recent/documents` is a single key for the whole origin,
  // which makes the loss concrete: one tab's recents list replaces the other's
  // outright. A Web Lock decides who owns the store; the tab that does not own
  // it is memory-only and says so.
  //
  // A second *tab*, not a second browser: `launchChrome` twice gives two
  // profiles, which share no IndexedDB and contend for no lock, so the
  // collaboration checks above could not see any of this.

  /** Reads one value out of the shared store, as a byte length. */
  const storedSize = (page, key) =>
    page.evaluate(
      (wanted) =>
        new Promise((resolve) => {
          const opened = indexedDB.open("opendoc");
          opened.onerror = () => resolve(null);
          opened.onsuccess = () => {
            const database = opened.result;
            if (![...database.objectStoreNames].includes("volume")) {
              database.close();
              resolve(null);
              return;
            }
            const request = database
              .transaction("volume", "readonly")
              .objectStore("volume")
              .get(wanted);
            request.onerror = () => {
              database.close();
              resolve(null);
            };
            request.onsuccess = () => {
              const value = request.result;
              database.close();
              resolve(value ? new Uint8Array(value).length : null);
            };
          };
        }),
      key,
    );

  let secondTab = null;
  try {
    await check("a second tab is told it does not own durable storage", async () => {
      // The first tab booted long ago and holds the lock.
      const owner = await chrome.evaluate(() => window.__OPENDOC_STORAGE__?.report() ?? null);
      assert.ok(owner, "the first tab published no storage report");
      assert.equal(
        owner.persistent,
        true,
        `the first tab is not durable: ${JSON.stringify(owner)}`,
      );
      assert.equal(
        owner.notOwner,
        undefined,
        `the owning tab claimed it was not the owner: ${owner.notOwner}`,
      );

      secondTab = await chrome.newPage();
      await secondTab.goto(server.url);
      const second = await pollUntil(
        secondTab,
        "the second tab reports on storage",
        () => window.__OPENDOC_STORAGE__?.report() ?? null,
        null,
        { tries: 300, delayMs: 100 },
      );
      assert.equal(
        second.persistent,
        false,
        `two tabs both claimed to own durable storage, which is the clobber: ${JSON.stringify(second)}`,
      );
      assert.match(
        String(second.notOwner),
        /another tab owns durable storage/,
        `the second tab did not say why it is not durable: ${JSON.stringify(second)}`,
      );

      // And the user is actually told, rather than it living in an object.
      // `storage_ready`'s report used to be discarded at the await.
      const notice = await pollUntil(
        secondTab,
        "the second tab tells the user",
        () => document.querySelector("[data-storage-notice]")?.textContent ?? null,
        null,
        { tries: 200, delayMs: 100 },
      );
      assert.match(
        notice,
        /not being saved/,
        `the second tab kept its storage state to itself: ${JSON.stringify(notice)}`,
      );
    });

    await check("a second tab cannot overwrite the first tab's recents list", async () => {
      // The owning tab has saved documents in the checks above, so there is a
      // real recents list in the store to defend.
      const before = await storedSize(chrome, "recent/documents");
      assert.ok(before, "the first tab has no durable recents list to defend");

      // Now make the second tab do the thing that clobbers: save a document of
      // its own under a different name. That rewrites `recent/documents` on
      // its volume, and `recent/documents` is one key for the whole origin.
      await openBlankDocument(secondTab);
      await typeIntoDocument(secondTab, "second tab");
      await secondTab.evaluate(() => {
        const group = document.querySelector('details.menu-group[data-menu="File"]');
        if (group) group.open = true;
      });
      await secondTab.click('details.menu-group[data-menu="File"] [data-action="save"]');
      await pollUntil(
        secondTab,
        "the second tab's save prompt",
        () => {
          const dialog = [...document.querySelectorAll("dialog.modal[open]")].find(
            (node) => node.querySelector("h2")?.textContent === "Save",
          );
          if (!dialog) return null;
          const form = dialog.querySelector("form");
          const control = form.elements.namedItem("path");
          if (control) control.value = "second-tab-repo";
          form.requestSubmit();
          return true;
        },
        null,
        { tries: 100, delayMs: 100 },
      );
      await settle(secondTab);
      // Long enough for a flush to have happened if one were going to.
      await secondTab.evaluate(() => new Promise((resolve) => setTimeout(resolve, 1000)));

      // The second tab holds no database handle, so nothing it wrote can have
      // reached the store: the owning tab's list is untouched, and the second
      // tab's repository is not in there at all.
      const after = await storedSize(chrome, "recent/documents");
      assert.equal(
        after,
        before,
        "a tab that does not own storage still wrote to it, replacing the owner's recents list",
      );
      const keys = await storedKeys();
      assert.ok(
        !keys.some((key) => key.startsWith("repositories/second-tab-repo/")),
        "a tab that does not own storage wrote a whole repository into it",
      );

      // It is queueing, not discarding: the work is held, ready for a
      // promotion if the owning tab ever closes.
      const waiting = await secondTab.evaluate(
        () => window.__OPENDOC_STORAGE__?.status() ?? null,
      );
      assert.equal(waiting.persistent, false, JSON.stringify(waiting));
      assert.ok(
        waiting.pending > 0,
        `the waiting tab dropped its work instead of holding it: ${JSON.stringify(waiting)}`,
      );
    });
  } finally {
    if (secondTab) await secondTab.close();
  }

  await check("the number Rust writes is the number the browser would count", async () => {
    // Ordered-list numbering used to be implemented twice — once in
    // `opendoc-render` keyed by `(list_id, level)`, once in `opendoc-layout`
    // keyed by depth — and both counted *bulleted* items into the ordered
    // counter. A run of two bullets followed by ordered items therefore wrote
    // `value="3"` on an item Chrome, given the same markup with `value=`
    // stripped, calls `1`. The two implementations are now one
    // (`opendoc_layout::lists::ListNumbering`), and the invariant this asserts
    // is the one that makes the markup honest: **the number we write is the
    // number a browser would count unaided.**
    //
    // This is deliberately not a render-vs-layout agreement check. Two copies
    // of the same wrong rule agree perfectly; the DOM Chrome actually built is
    // the independent answer.
    await openBlankDocument(chrome);
    await chrome.type("bullet one");
    await settle(chrome);
    await menuAction("Format", "style:list:bullet");
    await settle(chrome);
    await caretAtEndOfDocument();
    await chrome.press("Enter");
    await chrome.type("bullet two");
    await settle(chrome);
    await caretAtEndOfDocument();
    await chrome.press("Enter");
    await chrome.type("ordered one");
    await settle(chrome);
    await menuAction("Format", "style:list:ordered");
    await settle(chrome);
    await caretAtEndOfDocument();
    await chrome.press("Enter");
    await chrome.type("ordered two");
    await settle(chrome);

    const counted = await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const lists = [...host.querySelectorAll("ol.doc-list")];
      const wrong = [];
      let items = 0;
      for (const list of lists) {
        const children = [...list.children].filter((node) => node.tagName === "LI");
        children.forEach((item, index) => {
          items += 1;
          const stated = Number(item.getAttribute("value"));
          if (stated !== index + 1) {
            wrong.push({ stated, expected: index + 1, text: item.textContent.trim() });
          }
        });
      }
      return { lists: lists.length, items, wrong };
    });

    // Without this the check passes on a document that grew no ordered list
    // at all, which is the failure mode it exists to catch.
    assert.ok(
      counted.lists >= 1 && counted.items >= 2,
      `no ordered list was built, so this check proves nothing: ${JSON.stringify(counted)}`,
    );
    assert.deepEqual(
      counted.wrong,
      [],
      "an ordered item states a number the browser would not count for it: " +
        JSON.stringify(counted.wrong),
    );

    // And the bullets before it are genuinely a separate wrapper carrying no
    // number — the specific thing that used to spend the ordinals.
    const bullets = await chrome.evaluate(() => {
      const host = document.querySelector('[contenteditable="true"]');
      const lists = [...host.querySelectorAll("ul.doc-list:not(.doc-checklist)")];
      return {
        wrappers: lists.length,
        numbered: lists.flatMap((list) =>
          [...list.children]
            .filter((node) => node.tagName === "LI" && node.hasAttribute("value"))
            .map((node) => node.textContent.trim()),
        ),
      };
    });
    assert.ok(bullets.wrappers >= 1, "the bulleted run did not become its own wrapper");
    assert.deepEqual(
      bullets.numbered,
      [],
      `a bulleted item carries a number the browser will not draw: ${JSON.stringify(bullets.numbered)}`,
    );
  });

  await check("a stated cell border reaches the screen, including when it is none", async () => {
    // `opendoc-layout` used to stroke a constant for every cell — no colour on
    // `PaintItem::Stroke` at all, so the PDF printed black where the screen
    // draws grey, and every interior boundary twice. It now resolves real
    // borders and reproduces CSS 2.1 §17.6.2.1's collapse rules.
    //
    // What this check is for: proving a stated border reaches the *screen* at
    // all, which nothing else covers. The collapse winner is deliberately NOT
    // asserted here — under `border-collapse: collapse` Chrome's
    // `getComputedStyle` reports each cell's own border, not the boundary's
    // winner, so a "wider wins" assertion read this way would be asserting the
    // input back to itself. That rule is proven in Rust, and in pixels by
    // rasterising the PDF at 300dpi.
    await insertTable(2, 2);

    await caretInCell(0, 0);
    await menuAction("Table", "table-cell-border");
    await answerNamedPrompt("Cell border", {
      edge: "end",
      style: "solid",
      points: "3",
      color: "#0000ff",
    });
    await settle(chrome);

    await caretInCell(1, 0);
    for (const edge of ["top", "bottom", "start", "end"]) {
      await menuAction("Table", "table-cell-border");
      await answerNamedPrompt("Cell border", {
        edge,
        style: "none",
        points: "0",
        color: "#000000",
      });
      await settle(chrome);
    }

    const seen = await chrome.waitUntil(
      "the stated borders reach computed style",
      () => {
        const table = document.querySelector('[contenteditable="true"] table[data-block-id]');
        const rows = table?.querySelectorAll("tbody > tr");
        if (!rows || rows.length < 2) return null;
        const read = (r, c) => {
          const cell = rows[r].querySelectorAll("td")[c];
          const s = window.getComputedStyle(cell);
          return {
            top: [s.borderTopStyle, s.borderTopWidth, s.borderTopColor],
            right: [s.borderRightStyle, s.borderRightWidth, s.borderRightColor],
            bottom: [s.borderBottomStyle, s.borderBottomWidth, s.borderBottomColor],
            left: [s.borderLeftStyle, s.borderLeftWidth, s.borderLeftColor],
          };
        };
        const a = read(0, 0);
        // Wait until the stated edge has actually landed.
        if (a.right[0] !== "solid" || a.right[2] !== "rgb(0, 0, 255)") return null;
        return { a, off: read(1, 0), plain: read(0, 1) };
      },
      { tries: 80, delayMs: 100 },
    );

    // A stated border reaches the screen with its own style, width and colour.
    assert.deepEqual(
      seen.a.right,
      ["solid", "4px", "rgb(0, 0, 255)"],
      `the stated 3pt blue edge did not reach computed style: ${JSON.stringify(seen.a.right)}`,
    );

    // `none` reaches as none on the cell that states it. Whether a *line* is
    // drawn there still depends on the neighbour, which is the collapse rule.
    for (const edge of ["top", "right", "bottom", "left"]) {
      assert.equal(
        seen.off[edge][0],
        "none",
        `cell (1,0) states every edge none, but ${edge} computed ${JSON.stringify(seen.off[edge])}`,
      );
      assert.equal(
        seen.off[edge][1],
        "0px",
        `a none border must have zero used width, ${edge} computed ${seen.off[edge][1]}`,
      );
    }

    // And an untouched cell still carries the projected default grid rather
    // than nothing — so the check above is not passing because borders stopped
    // working altogether.
    assert.equal(
      seen.plain.top[0],
      "solid",
      `an untouched cell lost the default grid: ${JSON.stringify(seen.plain.top)}`,
    );
    assert.equal(
      seen.plain.top[2],
      "rgb(153, 153, 153)",
      `the default grid is not the projected colour: ${JSON.stringify(seen.plain.top)}`,
    );
  });

  const shot = await chrome.screenshot(join(artifacts, "e2e-screenshot.png"));
  results.push(`  ..   screenshot written to ${shot}`);
} catch (error) {
  failed += 1;
  results.push(`  FAIL e2e harness (${error.message.split("\n")[0]})`);
} finally {
  // A failed secondary-browser close must not orphan the service, primary
  // Chrome, or static server.  Each cleanup is bounded by its own CDP/service
  // shutdown and all failures are reported alongside the test result.
  const cleanup = await Promise.allSettled([
    bobChrome?.close(),
    collabService?.close(),
    chrome?.close(),
    server?.close(),
  ]);
  for (const result of cleanup) {
    if (result.status === "rejected") {
      failed += 1;
      results.push(`  FAIL e2e cleanup (${String(result.reason?.message ?? result.reason).split("\n")[0]})`);
    }
  }
}

console.log("\n== desktop e2e (real Chrome) ==");
for (const line of results) console.log(line);

if (failed > 0) {
  console.error(`\n${failed} e2e check(s) failed`);
  process.exit(1);
}
console.log("\ndesktop e2e passed");
