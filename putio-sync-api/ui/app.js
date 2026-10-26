// Shell: hash router, auth gate, polling, toasts and confirmations.
// Pages export { mount(el, params), unmount() } and import helpers from here,
// api.js and format.js; they never import each other.

import { getToken, logout, onAuthExpired } from "./api.js";
import * as status from "./pages/status.js";
import * as queue from "./pages/queue.js";
import * as putio from "./pages/putio.js";
import * as transfers from "./pages/transfers.js";
import * as history from "./pages/history.js";
import * as settings from "./pages/settings.js";
import * as login from "./pages/login.js";

const PAGES = { status, queue, putio, transfers, history, settings, login };
const TITLES = {
  status: "Status",
  queue: "Queue",
  putio: "Put.io",
  transfers: "Transfers",
  history: "History",
  settings: "Settings",
  login: "Sign in",
};
const PAGE_KEY = "putio-sync.page";
const BACKOFF_MS = 10000;
const TOAST_MS = 3000;

const view = document.getElementById("view");
const banner = document.getElementById("banner");

let current = null;
let polls = new Set();
let loginParams = {};

// ---- DOM helper -------------------------------------------------------------

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

function append(el, children) {
  for (const child of children) {
    if (child == null || child === false) continue;
    if (Array.isArray(child)) append(el, child);
    else el.append(child instanceof Node ? child : String(child));
  }
}

// ---- Storage ----------------------------------------------------------------

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

// ---- Router -----------------------------------------------------------------

function parse(hash) {
  const parts = hash.replace(/^#\/?/, "").split("/").filter(Boolean);
  const name = PAGES[parts[0]] ? parts[0] : "status";
  return { name, path: PAGES[parts[0]] ? parts.slice(1) : [] };
}

/** Go to `hash` (e.g. "#/queue"); re-renders when it is already the current route. */
export function navigate(hash) {
  if (window.location.hash === hash) route();
  else window.location.hash = hash;
}

function route() {
  const { name, path } = parse(window.location.hash);
  if (name !== "login" && !getToken()) {
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
  mount(name, { path });
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

  document.body.classList.toggle("signed-out", name === "login");
  for (const a of document.querySelectorAll("#nav a")) {
    if (a.dataset.page === name) a.setAttribute("aria-current", "page");
    else a.removeAttribute("aria-current");
  }
  document.title = `${TITLES[name]} · putio-sync`;
  document.getElementById("signout").hidden = name === "login";

  view.replaceChildren();
  current = PAGES[name];
  current.mount(view, params);
}

/** Show the sign-in page; `params` may carry `reason` and `retryAfter`. */
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

// ---- Polling ----------------------------------------------------------------

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

// ---- Toasts and confirmations -----------------------------------------------

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

// ---- Boot -------------------------------------------------------------------

onAuthExpired((err) => {
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

if (!window.location.hash || window.location.hash === "#" || window.location.hash === "#/") {
  const last = remembered();
  window.location.replace(last && getToken() ? last : "#/status");
} else {
  route();
}
