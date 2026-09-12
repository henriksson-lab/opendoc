// The find bar.
//
// Matching itself is not here. Case folding, whole-word boundaries, regular
// expressions and — the part TypeScript kept getting wrong — matches that
// begin in one inline run and end in another are document semantics, so they
// live in `find_in_document` (crates/opendoc-app/src/find.rs). What is below
// is the bar's DOM: the query box, the toggles, the counter, the highlight,
// and the two buttons that dispatch a replace.
//
// The bar owns its own state. Nothing outside this module reads the query, the
// match list or the cursor into it; the dispatcher only names the four actions
// exported at the bottom.
import { escapeHtml, focusFieldAtEnd } from "./ui";
import { invoke } from "./invoke";
import type { AppFindMatch } from "./types";
import { state } from "./state";
import { edit, query } from "./shared";
import { runAction } from "./actions";

type FindToggles = { matchCase: boolean; wholeWord: boolean; regex: boolean };

let findQuery = "";
let findIndex = 0;
let showFind = false;
let findOptions: FindToggles = { matchCase: false, wholeWord: false, regex: false };

/** Latest answer from Rust, rendered synchronously by `renderFind`. */
let findResults: AppFindMatch[] = [];
let findError: string | null = null;
/** Discards the answer to a query the user has already typed past. */
let findGeneration = 0;

function wrapIndex(index: number, length: number): number {
  return ((index % length) + length) % length;
}

function findArgs(): { query: string; matchCase: boolean; wholeWord: boolean; regex: boolean } {
  return { query: findQuery, ...findOptions };
}

export async function refreshFind(): Promise<void> {
  if (!state.doc || !showFind) return;
  const generation = ++findGeneration;
  let matches: AppFindMatch[] = [];
  let error: string | null = null;
  if (findQuery) {
    try {
      matches = (await invoke("find_in_document", findArgs())).matches;
    } catch (failure) {
      // A half-typed regular expression is invalid on the way to a valid one,
      // so Rust's refusal belongs in the find bar, not in the error banner.
      error = failure instanceof Error ? failure.message : String(failure);
    }
  }
  if (generation !== findGeneration) return;
  findResults = matches;
  findError = error;
  findIndex = findResults.length === 0 ? 0 : wrapIndex(findIndex, findResults.length);
  renderFind();
}

function findStatusText(): string {
  if (findError) return "Invalid pattern";
  if (!findQuery) return "";
  if (findResults.length === 0) return "No matches";
  return `${wrapIndex(findIndex, findResults.length) + 1} of ${findResults.length}`;
}

export function renderFind(): void {
  const bar = query("[data-find]");
  if (!bar) return;
  bar.hidden = !showFind;
  if (!showFind) return;
  if (!query("[data-find-input]", bar)) buildFindBar(bar);
  const count = query(".find-count", bar);
  if (!count) return;
  count.textContent = findStatusText();
  count.classList.toggle("invalid", findError !== null);
  if (findError) count.setAttribute("title", findError);
  else count.removeAttribute("title");
}

/**
 * Built once per shell, then only the counter is rewritten: rebuilding the
 * markup under a focused `<input>` would drop the caret mid-word.
 */
function buildFindBar(bar: HTMLElement): void {
  const toggle = (name: keyof FindToggles, label: string, title: string) =>
    `<label class="find-toggle" title="${escapeHtml(title)}"><input type="checkbox" data-find-option="${name}" aria-label="${escapeHtml(title)}"><span>${escapeHtml(label)}</span></label>`;
  bar.innerHTML = `
    <input type="search" data-find-input value="${escapeHtml(findQuery)}" placeholder="Find in document" aria-label="Find">
    <span class="find-count">${escapeHtml(findStatusText())}</span>
    <button type="button" data-action="find-prev" title="Previous match">↑</button>
    <button type="button" data-action="find-next" title="Next match">↓</button>
    ${toggle("matchCase", "Aa", "Match case")}
    ${toggle("wholeWord", "Word", "Whole word")}
    ${toggle("regex", ".*", "Regular expression")}
    <input type="text" data-replace-input placeholder="Replace with" aria-label="Replace with">
    <button type="button" data-action="replace-one">Replace</button>
    <button type="button" data-action="replace-all">Replace all</button>
    <button type="button" data-action="find-close" aria-label="Close">✕</button>`;
  const input = query<HTMLInputElement>("[data-find-input]", bar);
  if (input) {
    input.oninput = () => {
      findQuery = input.value;
      findIndex = 0;
      void refreshFind().then(highlightMatch);
    };
    input.onkeydown = (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        void runAction(event.shiftKey ? "find-prev" : "find-next");
      }
      if (event.key === "Escape") {
        void runAction("find-close");
      }
    };
  }
  bar.querySelectorAll<HTMLInputElement>("input[data-find-option]").forEach((option) => {
    const name = option.dataset.findOption as keyof FindToggles;
    option.checked = findOptions[name];
    option.onchange = () => {
      findOptions = { ...findOptions, [name]: option.checked };
      findIndex = 0;
      void refreshFind().then(highlightMatch);
    };
  });
}

function highlightMatch(): void {
  if (findResults.length === 0 || !state.editor) return;
  const match = findResults[wrapIndex(findIndex, findResults.length)];
  // Rust hands back a pair of editor positions, so a match that spans two
  // inline runs highlights as one selection with no arithmetic here.
  state.editor.setSelection({ anchor: match.start, focus: match.end });
}

// ---- The four actions the dispatcher routes here -----------------------------

export async function openFind(): Promise<void> {
  showFind = true;
  renderFind();
  await refreshFind();
  focusFieldAtEnd(query<HTMLInputElement>("[data-find-input]"));
}

export function closeFind(): void {
  showFind = false;
  renderFind();
  state.editor?.focus();
}

/** Moves the cursor through the match list, wrapping at either end. */
export function stepFind(delta: number): void {
  if (findResults.length === 0) return;
  findIndex = wrapIndex(findIndex + delta, findResults.length);
  highlightMatch();
  renderFind();
}

export async function replaceFromFindBar(all: boolean): Promise<void> {
  if (!findQuery || findResults.length === 0) return;
  const replacement = query<HTMLInputElement>("[data-replace-input]")?.value ?? "";
  // One command, so replace-all is one undoable edit rather than N, and
  // neither path re-derives positions the frontend cached.
  if (all) {
    await edit("replace_all_in_document", { ...findArgs(), replacement });
  } else {
    await edit("replace_match_in_document", {
      ...findArgs(),
      replacement,
      matchIndex: wrapIndex(findIndex, findResults.length),
    });
  }
}
