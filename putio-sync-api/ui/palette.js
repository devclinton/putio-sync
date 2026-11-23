// Command palette (feature 061): Cmd+K (Ctrl+K off macOS), or Search in the top bar, finds
// actions, pages, settings, queue items, Put.io files and local files from any signed-in
// page. Groups keep one order and hide when empty. Actions, pages and settings are in the
// page; the queue is fetched once per opening; the two file searches wait for a pause in
// typing, and a newer query cancels the older one. Actions that change things confirm as
// their buttons do. app.js listens for the shortcut and loads this module on first use.

import { get, post, syncNow } from "./api.js";
import { confirm, h, navigate, paletteBlocked, setNasPaused, toast, toastError } from "./app.js";
import { bytes } from "./format.js";
import { rank } from "./fuzzy.js";
import { tr } from "./i18n.js";
import { entryText } from "./settings/catalog.js";
import { settingsCatalog } from "./settings/groups.js";
import { GUIDES, KIND_LABELS } from "./guides.js";

const LIMIT = 8;
const WAIT_MS = 250;
const MIN_FILE_QUERY = 2;
const GROUPS = [
  ["actions", "Actions"],
  ["pages", "Pages"],
  ["settings", "Settings"],
  ["guides", "Guides"],
  ["queue", "Queue"],
  ["putio", "Put.io files"],
  ["local", "Local files"],
];

const ACTIONS = [
  { label: "Sync now", run: sync },
  { label: "Pause downloads", detail: "Local downloads", run: () => setNasPaused(true) },
  { label: "Resume downloads", detail: "Local downloads", run: () => setNasPaused(false) },
  { label: "Clear completed", detail: "Queue", run: clearCompleted },
  { label: "Check for updates", run: checkUpdates },
  // The top bar's own controls, so these do exactly what they do
  { label: "Toggle theme", detail: "Appearance", run: () => document.getElementById("mode").click() },
  { label: "Sign out", shown: () => !document.getElementById("signout").hidden, run: () => document.getElementById("signout").click() },
];

let dialog = null;
let input = null;
let list = null;
let shown = []; // the options on screen, in order
let active = 0;
let back = null; // what had the focus before opening
let queue = null; // the queue items, or null while loading or when it can't be read
let files = null; // { putio, local }: { q, state: "busy" | "done" | "failed", hits, truncated }
let timer = 0;
let ctl = null;
let opened = 0;

async function sync() {
  try {
    await syncNow();
    toast("Sync started");
  } catch (e) {
    if (e.status === 409) toast("Sync already running");
    else toastError(e);
  }
}

async function clearCompleted() {
  if (!(await confirm({ title: "Clear completed", text: "Remove completed items?", verb: "Remove" }))) return;
  try {
    const res = await post("/local/queue/clear");
    toast((res && res.message) || "Done");
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toastError(e);
  }
}

// The update card is on What's new; it reads the result of the check when it loads
async function checkUpdates() {
  try {
    await post("/update/check");
  } catch (e) {
    // 409: checked less than a minute ago, so the card already has the answer
    if (e.status === 409) toast("Checked for updates a moment ago");
    else if (e.status !== 401 && e.status !== 429) toastError(e);
  }
  navigate("#/news");
}

/** The words a result is matched on: the text shown, in English and in the language in use */
const words = (label, detail = "") => `${tr(label)} ${label} ${detail}`;

function pages() {
  const nav = [...document.querySelectorAll("#nav a")].map((a) => ({ label: a.textContent.trim(), href: a.getAttribute("href") }));
  return [...nav, { label: "What's new", href: "#/news" }];
}

// Results per group, best first. An empty query lists actions and pages to browse.

function results(q) {
  const out = {};
  const actions = ACTIONS.filter((a) => !a.shown || a.shown());
  const nav = pages().map((p) => ({ ...p, run: () => navigate(p.href) }));
  if (!q) {
    out.actions = actions.slice(0, LIMIT);
    out.pages = nav.slice(0, LIMIT);
    return out;
  }
  out.actions = rank(q, actions, (a) => words(a.label, a.detail), LIMIT);
  out.pages = rank(q, nav, (p) => words(p.label), LIMIT);
  out.settings = rank(q, settingsCatalog(), (e) => `${tr(e.label)} ${entryText(e)}`, LIMIT).map((e) => ({ label: e.label, detail: e.detail, run: () => navigate(e.href) }));
  out.guides = rank(q, GUIDES, (g) => `${words(g.title)} ${g.words}`, LIMIT).map((g) => ({
    label: g.title,
    detail: KIND_LABELS[g.kind],
    run: () => window.open(`https://devclinton.github.io/putio-sync/docs/${g.slug}.html`, "_blank", "noopener"),
  }));
  if (queue) {
    out.queue = rank(q, queue, (i) => i.filename, LIMIT).map((i) => ({ label: i.filename, detail: i.status, run: () => navigate(`#/queue?file=${i.file_id}`) }));
  }
  if (files && files.putio.q === q && files.putio.hits) {
    out.putio = best(q, files.putio.hits, (f) => f.name).map((f) => ({
      label: f.name,
      detail: f.is_folder ? "Folder" : bytes(f.size),
      run: () => navigate(`#/putio/${f.parent_id}?select=${encodeURIComponent(f.name)}`),
    }));
  }
  if (files && files.local.q === q && files.local.hits) {
    out.local = best(q, files.local.hits, (x) => x.name).map((x) => {
      const dir = x.path.includes("/") ? x.path.slice(0, x.path.lastIndexOf("/")) : "";
      const query = new URLSearchParams(dir ? { path: dir, select: x.name } : { select: x.name });
      return { label: x.name, detail: dir ? `${x.root_label} › ${dir}` : x.root_label, run: () => navigate(`#/files/${encodeURIComponent(x.root)}?${query}`) };
    });
  }
  return out;
}

/** Server matches ranked by the loose match, then the ones it doesn't score, up to LIMIT */
function best(q, hits, text) {
  const top = rank(q, hits, text, LIMIT);
  for (const x of hits) {
    if (top.length >= LIMIT) break;
    if (!top.includes(x)) top.push(x);
  }
  return top;
}

/** The line under a file group's heading: still searching, failed, or stopped early */
function note(group, q) {
  const f = files && files[group];
  if (!f || f.q !== q) return null;
  if (f.state === "busy") return "Searching…";
  if (f.state === "failed") return "Couldn't search";
  if (f.truncated) return "More not shown";
  return null;
}

function render() {
  const q = input.value.trim();
  const before = shown[active];
  const all = results(q);
  shown = [];
  const rows = [];
  for (const [id, title] of GROUPS) {
    const items = all[id] || [];
    const line = id === "putio" || id === "local" ? note(id, q) : null;
    // "More not shown" only goes with results; a search that found nothing just hides
    if (!items.length && (!line || line === "More not shown")) continue;
    const head = `palette-${id}`;
    rows.push(
      h(
        "li",
        { role: "group", "aria-labelledby": head },
        h("div", { class: "palette-head", id: head, text: title }),
        h(
          "ul",
          { role: "presentation" },
          items.map((item) => {
            const i = shown.push(item) - 1;
            return h(
              "li",
              { role: "option", id: `palette-opt-${i}`, class: "palette-item", "aria-selected": "false", dataset: { i: String(i) } },
              h("span", { class: "palette-label", text: item.label }),
              item.detail ? h("span", { class: "palette-detail muted", text: item.detail }) : null,
            );
          }),
          line ? h("li", { role: "presentation", class: "palette-note muted", text: line }) : null,
        ),
      ),
    );
  }
  if (!rows.length) rows.push(h("li", { role: "presentation", class: "palette-note muted", text: "Nothing matched." }));
  list.replaceChildren(...rows);
  // Keep the same result picked when results arrive; otherwise the best one
  const again = before ? shown.findIndex((x) => x.label === before.label && x.detail === before.detail) : -1;
  pick(again >= 0 ? again : 0, false);
}

function pick(i, scroll = true) {
  active = shown.length ? (i + shown.length) % shown.length : 0;
  for (const el of list.querySelectorAll("[role=option]")) {
    el.setAttribute("aria-selected", String(Number(el.dataset.i) === active));
  }
  const el = shown.length ? document.getElementById(`palette-opt-${active}`) : null;
  if (el) input.setAttribute("aria-activedescendant", el.id);
  else input.removeAttribute("aria-activedescendant");
  if (el && scroll) el.scrollIntoView({ block: "nearest" });
}

function choose(item) {
  if (!item) return;
  close();
  Promise.resolve(item.run()).catch(toastError);
}

// File searches: after a pause in typing, both at once; a newer query aborts the older one

function typed() {
  const q = input.value.trim();
  clearTimeout(timer);
  if (ctl) ctl.abort();
  ctl = null;
  if (q.length < MIN_FILE_QUERY) {
    files = null;
    return render();
  }
  files = { putio: { q, state: "busy" }, local: { q, state: "busy" } };
  render();
  timer = setTimeout(() => search(q), WAIT_MS);
}

function search(q) {
  const mine = (ctl = new AbortController());
  const param = encodeURIComponent(q);
  const run = (group, path, read) =>
    get(path, mine.signal).then(
      (res) => {
        if (mine !== ctl) return;
        files[group] = { q, state: "done", ...read(res) };
        render();
      },
      (e) => {
        if (mine !== ctl || mine.signal.aborted) return;
        files[group] = { q, state: "failed" };
        render();
      },
    );
  run("putio", `/putio/files/search?q=${param}`, (r) => ({ hits: r.files || [] }));
  run("local", `/local/files/search?q=${param}`, (r) => ({ hits: r.hits || [], truncated: Boolean(r.truncated) }));
}

function build() {
  input = h("input", {
    class: "input palette-input",
    type: "search",
    role: "combobox",
    placeholder: "Search pages, settings, files and actions",
    "aria-label": "Search pages, settings, files and actions",
    "aria-expanded": "true",
    "aria-controls": "palette-list",
    "aria-autocomplete": "list",
    autocomplete: "off",
    spellcheck: "false",
  });
  list = h("ul", { id: "palette-list", class: "palette-list", role: "listbox", "aria-label": "Results" });
  const keys = h("p", { class: "palette-keys muted small", text: "↑ ↓ to move · Enter to open · Esc to close" });
  dialog = h("dialog", { id: "palette", class: "palette", "aria-label": "Command palette" }, h("div", { class: "palette-box" }, input, list, keys));
  document.body.append(dialog);

  input.addEventListener("input", typed);
  input.addEventListener("keydown", (ev) => {
    if (ev.key === "ArrowDown" || ev.key === "ArrowUp") {
      ev.preventDefault();
      pick(active + (ev.key === "ArrowDown" ? 1 : -1));
    } else if (ev.key === "Enter" && !ev.isComposing) {
      ev.preventDefault();
      choose(shown[active]);
    }
  });
  list.addEventListener("mousemove", (ev) => {
    const el = ev.target.closest("[role=option]");
    if (el && Number(el.dataset.i) !== active) pick(Number(el.dataset.i), false);
  });
  list.addEventListener("click", (ev) => {
    const el = ev.target.closest("[role=option]");
    if (el) choose(shown[Number(el.dataset.i)]);
  });
  // A click on the backdrop lands on the dialog itself, outside the box
  dialog.addEventListener("click", (ev) => ev.target === dialog && close());
  dialog.addEventListener("close", () => {
    clearTimeout(timer);
    if (ctl) ctl.abort();
    ctl = null;
    if (back && back.isConnected) back.focus({ preventScroll: true });
    back = null;
  });
}

export function open() {
  if (paletteBlocked()) return;
  if (!dialog) build();
  if (dialog.open) return void input.select();
  back = document.activeElement;
  input.value = "";
  files = null;
  queue = null;
  const mine = ++opened;
  get("/local/queue").then(
    (res) => {
      if (mine !== opened || !dialog.open) return;
      queue = (res && res.items) || [];
      render();
    },
    () => {}, // no database, or it can't be read: the Queue group is left out
  );
  render();
  dialog.showModal();
  input.focus();
}

export function close() {
  if (dialog && dialog.open) dialog.close();
}
