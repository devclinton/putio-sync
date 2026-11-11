// Shell: hash router, auth gate, polling, toasts and confirmations.
// Pages export { mount(el, params), unmount() } and import helpers from here,
// api.js and format.js; they never import each other.

import { authStatus, getIdentity, getToken, logout, onAuthExpired, post } from "./api.js";
import { choice, setLanguage, tr } from "./i18n.js";
import * as status from "./pages/status.js";
import * as active from "./pages/active.js";
import * as queue from "./pages/queue.js";
import * as putio from "./pages/putio.js";
import * as files from "./pages/files.js";
import * as transfers from "./pages/transfers.js";
import * as history from "./pages/history.js";
import * as stats from "./pages/stats.js";
import * as settings from "./pages/settings.js";
import * as doctor from "./pages/doctor.js";
import * as logs from "./pages/logs.js";
import * as login from "./pages/login.js";
import * as setup from "./pages/setup.js";
import * as news from "./pages/news.js";

const PAGES = { status, active, queue, putio, files, transfers, history, stats, doctor, logs, settings, login, setup, news };
const TITLES = {
  status: "Status",
  active: "Active",
  queue: "Queue",
  putio: "Put.io",
  files: "Local files",
  transfers: "Transfers",
  history: "History",
  stats: "Stats",
  doctor: "Doctor",
  logs: "Logs",
  settings: "Settings",
  login: "Sign in",
  setup: "Set up",
  news: "What's new",
};
const PAGE_KEY = "putio-sync.page";
const BACKOFF_MS = 10000;
const TOAST_MS = 3000;

const view = document.getElementById("view");
const banner = document.getElementById("banner");

let current = null;
let currentName = "status";
let polls = new Set();
let loginParams = {};
// Set at boot when the daemon waits for its first-run setup (feature 013)
let setupMode = false;
// What's new (feature 017) is checked once per page load, on the first signed-in page;
// a failed check (an expired token, say) is tried again on the next one
let newsChecked = false;

// DOM helper

/**
 * Build an element. `attrs` keys: `class`, `text`, `on<event>` handlers, `dataset`,
 * boolean or string attributes. Children are nodes, strings or arrays; null is skipped.
 * Text always goes through text nodes, never HTML.
 */
export function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs || {})) {
    if (value == null || value === false) continue;
    if (key === "class") el.className = value;
    else if (key === "text") el.textContent = value;
    else if (key === "dataset") Object.assign(el.dataset, value);
    else if (key.startsWith("on")) el.addEventListener(key.slice(2), value);
    else if (key === "style") el.style.cssText = value; // CSSOM, so a strict CSP allows it
    else el.setAttribute(key, value === true ? "" : value);
  }
  append(el, children);
  return el;
}

/** A link to the docs book's `page`, at heading `id` (055) */
export const docs = (page, id, text = "Docs") => h("a", { href: `https://devclinton.github.io/putio-sync/docs/${page}.html${id ? "#" + id : ""}`, target: "_blank", rel: "noopener" }, text);

// Replace an element's children like `h` adds them: null and false are skipped and
// arrays are flattened. The DOM's own `replaceChildren` shows null as the text "null".
export function fill(el, ...children) {
  el.replaceChildren();
  append(el, children);
}

function append(el, children) {
  for (const child of children) {
    if (child == null || child === false) continue;
    if (Array.isArray(child)) append(el, child);
    else el.append(child instanceof Node ? child : String(child));
  }
}

// Storage

function remember(hash) {
  try {
    window.localStorage.setItem(PAGE_KEY, hash);
  } catch {
    // storage unavailable
  }
}

function remembered() {
  try {
    return window.localStorage.getItem(PAGE_KEY);
  } catch {
    return null;
  }
}

// Router

/** "#/stats/x?range=30d" → { name: "stats", path: ["x"], query: URLSearchParams } */
function parse(hash) {
  const [route, search = ""] = hash.replace(/^#\/?/, "").split("?", 2);
  const parts = route.split("/").filter(Boolean);
  const name = PAGES[parts[0]] ? parts[0] : "status";
  return { name, path: PAGES[parts[0]] ? parts.slice(1) : [], query: new URLSearchParams(search) };
}

/** Go to `hash` (e.g. "#/queue"); re-renders when it is already the current route. */
export function navigate(hash) {
  if (window.location.hash === hash) route();
  else window.location.hash = hash;
}

function route() {
  if (setupMode) {
    if (current !== setup) mount("setup", {});
    return;
  }
  const { name, path, query } = parse(window.location.hash);
  if (name !== "login" && !getToken() && !getIdentity()) {
    showLogin({});
    return;
  }
  if (name === "login") {
    const params = loginParams;
    loginParams = {};
    mount("login", params);
    return;
  }
  remember(window.location.hash || "#/status");
  // A page with `update` shows another address of itself in place (Settings sections, 046)
  if (current === PAGES[name] && current.update) return void current.update({ path, query });
  mount(name, { path, query });
  if (!newsChecked) {
    newsChecked = true;
    news.checkNews().then((ok) => {
      newsChecked = ok;
      if (ok) news.checkUpdate();
    });
  }
}

function mount(name, params) {
  if (current) {
    try {
      current.unmount();
    } catch (e) {
      console.error(e);
    }
  }
  for (const p of polls) p.stop();
  polls = new Set();
  setBanner(false);

  document.body.classList.toggle("signed-out", name === "login" || name === "setup");
  for (const a of document.querySelectorAll("#nav a")) {
    if (a.dataset.page === name) a.setAttribute("aria-current", "page");
    else a.removeAttribute("aria-current");
  }
  currentName = name;
  showTitle();
  // Signed in by Tailscale only: there is nothing to sign out of here
  document.getElementById("signout").hidden = name === "login" || name === "setup" || (!getToken() && Boolean(getIdentity()));
  showWho();

  view.replaceChildren();
  enter();
  current = PAGES[name];
  current.mount(view, params);
}

function showTitle() {
  document.title = `${tr(TITLES[currentName])} · putio-sync`;
}
document.addEventListener("languagechange", showTitle);

// One entrance per page change; app.css only animates it without reduced motion.
let enterTimer = null;
function enter() {
  clearTimeout(enterTimer);
  view.classList.add("enter");
  enterTimer = setTimeout(() => view.classList.remove("enter"), 500);
}

/** Show the sign-in page; `params` may carry `reason` and `retryAfter`. */
/** "login (Tailscale)" in the top bar when Tailscale sign-in let this browser in */
function showWho() {
  const who = document.getElementById("who");
  const id = getIdentity();
  who.hidden = !id || Boolean(getToken());
  who.textContent = id ? `${id.login} (Tailscale)` : "";
  who.title = id ? `Signed in by Tailscale as ${id.name}` : "";
}

function showLogin(params) {
  loginParams = params;
  if (window.location.hash === "#/login") route();
  else window.location.hash = "#/login";
}

/** Where to go after signing in: the last page visited, else the status page. */
export function afterLogin() {
  const last = remembered();
  navigate(last && !last.startsWith("#/login") ? last : "#/status");
}

/**
 * An empty state: what would be here, and how it gets filled. Use it in place of
 * an empty table or list.
 */
export function empty(title, hint = null) {
  return h("div", { class: "empty" }, h("strong", { text: title }), hint ? h("p", { text: hint }) : null);
}

// Appearance

const MODE_LABELS = { system: "System", light: "Light", dark: "Dark" };
const modeButton = document.getElementById("mode");

function showMode() {
  const label = MODE_LABELS[window.appearance.get().mode];
  modeButton.firstElementChild.textContent = label;
  modeButton.setAttribute("aria-label", `Appearance: ${label}`);
}

let themingTimer = null;
document.addEventListener("appearancechange", () => {
  showMode();
  // Fade colours across the page for this change only
  clearTimeout(themingTimer);
  document.documentElement.classList.add("theming");
  themingTimer = setTimeout(() => document.documentElement.classList.remove("theming"), 350);
});

modeButton.addEventListener("click", () => {
  const modes = window.appearance.MODES;
  const next = modes[(modes.indexOf(window.appearance.get().mode) + 1) % modes.length];
  window.appearance.set({ mode: next });
});
showMode();

// Polling

function setBanner(shown) {
  banner.hidden = !shown;
}

/**
 * Call `fn` now and every `ms` until the page unmounts. Pauses while the tab is hidden.
 * A network error (status 0) shows the disconnected banner and slows to 10 s until the
 * next success. Returns { now(), stop() }; `now()` runs it at once (after an action).
 */
export function poll(fn, ms) {
  const p = { timer: null, running: false, stopped: false };
  p.tick = async () => {
    clearTimeout(p.timer);
    p.timer = null;
    if (p.stopped || p.running || document.hidden) return;
    p.running = true;
    let delay = ms;
    try {
      await fn();
      setBanner(false);
    } catch (e) {
      if (e && e.network) {
        setBanner(true);
        delay = BACKOFF_MS;
      } else if (e && (e.status === 401 || e.status === 429)) {
        p.stopped = true;
      } else {
        console.error(e);
      }
    }
    p.running = false;
    if (!p.stopped && !document.hidden) p.timer = setTimeout(p.tick, delay);
  };
  p.now = () => p.tick();
  p.stop = () => {
    p.stopped = true;
    clearTimeout(p.timer);
    polls.delete(p);
  };
  polls.add(p);
  p.tick();
  return { now: p.now, stop: p.stop };
}

document.addEventListener("visibilitychange", () => {
  if (document.hidden) return;
  for (const p of polls) if (!p.timer) p.tick();
});

// Toasts and confirmations

/**
 * Show a short message. `kind` is "ok" (gone after 3 s) or "error" (stays until
 * dismissed). `detail` is an optional second line.
 */
export function toast(text, kind = "ok", detail = null) {
  const close = h("button", { type: "button", "aria-label": "Dismiss", text: "×" });
  const el = h(
    "div",
    { class: `toast ${kind}`, role: kind === "error" ? "alert" : "status" },
    h("p", {}, text, detail ? h("small", { text: detail }) : null),
    close,
  );
  close.addEventListener("click", () => el.remove());
  document.getElementById("toasts").append(el);
  if (kind !== "error") setTimeout(() => el.remove(), TOAST_MS);
}

/**
 * Turn the Pause NAS downloads switch on or off (feature 039) and say so. Resolves true
 * when the switch changed; an error is toasted and resolves false.
 */
export async function setNasPaused(on) {
  try {
    await post(`/local/queue/${on ? "pause" : "resume"}`);
    toast(on ? "NAS downloads paused" : "NAS downloads resumed");
    return true;
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toast(`Couldn't ${on ? "pause" : "resume"} NAS downloads`, "error", e.details ? `${e.error}: ${e.details}` : e.error);
    return false;
  }
}

/**
 * The banner shown while the Pause NAS downloads switch is on, with a Resume button.
 * `update(paused)` shows or hides it; `onChange` runs after the switch is turned off here.
 */
export function nasBanner(onChange = () => {}) {
  const button = h("button", { type: "button", class: "btn btn-small", onclick: resume }, "Resume");
  const el = h("div", { class: "notice nas-paused", role: "status", hidden: true }, h("strong", { text: "NAS downloads paused" }), h("span", { text: " Nothing new starts on the NAS; Put.io keeps downloading." }), button);
  async function resume() {
    button.disabled = true;
    const done = await setNasPaused(false);
    if (button.isConnected) button.disabled = false;
    if (done) onChange();
  }
  return {
    el,
    update(paused) {
      el.hidden = !paused;
    },
  };
}

/** Toast an API error with its `error` and `details`. */
export function toastError(err) {
  toast(err && err.error ? err.error : String(err), "error", err && err.details);
}

/**
 * Ask before a destructive action. Resolves true when `verb` is chosen.
 * Cancel has the focus, so Enter or Escape keeps things as they are.
 */
export function confirm({ title, text, verb }) {
  const dialog = document.getElementById("confirm");
  document.getElementById("confirm-title").textContent = title;
  document.getElementById("confirm-text").textContent = text || "";
  document.getElementById("confirm-ok").textContent = verb || "Confirm";
  dialog.returnValue = "cancel";
  return new Promise((resolve) => {
    dialog.addEventListener("close", () => resolve(dialog.returnValue === "ok"), {
      once: true,
    });
    dialog.showModal();
    document.getElementById("confirm-cancel").focus();
  });
}

// Boot

onAuthExpired(async (err) => {
  // A Tailscale identity that was let in may have been taken off the allow-list
  if (!getToken() && getIdentity()) await authStatus().catch(() => {});
  if (err.status === 429) {
    showLogin({ reason: "Too many attempts.", retryAfter: err.retryAfter });
  } else {
    showLogin({ reason: "Your session has expired. Sign in again." });
  }
});

document.getElementById("signout").addEventListener("click", () => {
  logout();
  showLogin({ reason: "Signed out." });
});

window.addEventListener("hashchange", route);

// The saved or browser language (feature 051), before the first page shows
await setLanguage(choice(), false);

// A daemon without a usable config serves the setup wizard instead
try {
  setupMode = (await authStatus()).setup_required === true;
} catch {
  // unreachable: route as usual and let the pages report it
}

if (setupMode) {
  route();
} else if (!window.location.hash || window.location.hash === "#" || window.location.hash === "#/") {
  const last = remembered();
  window.location.replace(last && (getToken() || getIdentity()) ? last : "#/status");
} else {
  route();
}
