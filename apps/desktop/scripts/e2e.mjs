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
// Requires `npm run build` first.
import assert from "node:assert/strict";
import { join } from "node:path";
import { launchChrome, startStaticServer } from "./cdp.mjs";

const desktopRoot = new URL("..", import.meta.url).pathname;
const PORT = Number(process.env.E2E_PORT ?? 10185);
const CDP_PORT = Number(process.env.E2E_CDP_PORT ?? 9399);
const artifacts = process.env.E2E_ARTIFACTS ?? desktopRoot;

const results = [];
let failed = 0;

async function check(name, fn) {
  try {
    await fn();
    results.push(`  ok   ${name}`);
  } catch (error) {
    failed += 1;
    results.push(`  FAIL ${name}\n       ${error.message.split("\n")[0]}`);
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

const server = await startStaticServer({ port: PORT, root: join(desktopRoot, "dist") });
const chrome = await launchChrome({ port: CDP_PORT });

try {
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
      select.value = "multiple:2000";
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
      answerPrompt("Header", { text: "Chapter", field: "page-number", alignment: "center" }),
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
          width: rect.width,
          blocks: document.querySelectorAll('[contenteditable="true"] figure.doc-image').length,
        };
      }),
    );
    assert.equal(image.blocks, 1, `the drop produced ${image.blocks} image blocks`);
    assert.equal(image.placement, "block", "a new image is placed on its own line");
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

    await caretInCell(0, 0);
    await menuAction("Table", "table-merge-cells");
    await answerNamedPrompt("Merge cells", { rows: "2", columns: "2" });
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

  const shot = await chrome.screenshot(join(artifacts, "e2e-screenshot.png"));
  results.push(`  ..   screenshot written to ${shot}`);
} finally {
  await chrome.close();
  await server.close();
}

console.log("\n== desktop e2e (real Chrome) ==");
for (const line of results) console.log(line);

if (failed > 0) {
  console.error(`\n${failed} e2e check(s) failed`);
  process.exit(1);
}
console.log("\ndesktop e2e passed");
