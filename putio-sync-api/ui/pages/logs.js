// Logs page (feature 047): recent daemon lines from /logs, then a live tail read from
// /logs/stream (fetch, so it can carry the token; EventSource can't). Level, module and
// text filters live in the page address. Pause holds new lines with a count; Clear view
// hides what is shown in this browser only. The level panel sets the overall or a
// module's level for 30 minutes or until restart. Values that name an item link to it.

import { del, get, getToken, put } from "../api.js";
import { fill, h, toast, toastError } from "../app.js";
import { dateTime } from "../format.js";

const KEYS = ["level", "target", "q"];
const LEVELS = ["error", "warn", "info", "debug", "trace"];
const MAX_SHOWN = 2000;
const RETRY_MS = 3000;
const DEBOUNCE_MS = 300;
const BADGE = { ERROR: "bad", WARN: "warn", INFO: "ok", DEBUG: "active", TRACE: "" };
const DIGITS = /^\d+$/;
const NAME = /^[\w.-]+$/;
// Field → where its value links
const LINKS = {
  file_id: [DIGITS, (v) => `https://app.put.io/files/${v}`],
  folder_id: [DIGITS, (v) => `#/putio/${v}`],
  parent_id: [DIGITS, (v) => `#/putio/${v}`],
  transfer_id: [DIGITS, () => "#/transfers"],
  hook: [NAME, () => "#/settings/integrations?field=card-hooks"],
  plugin: [NAME, () => "#/settings/integrations?field=card-plugins"],
  run_id: [DIGITS, () => "#/history"],
};

let f = {};
let els = null;
let shown = [];
let waiting = [];
let paused = false;
let last = 0;
let tail = null;
let typing = null;
let retry = null;

export function mount(el, params = {}) {
  const q = params.query || new URLSearchParams();
  f = Object.fromEntries(KEYS.map((k) => [k, q.get(k) || ""]));
  shown = [];
  waiting = [];
  paused = false;
  const input = (key, attrs) => h("input", { class: "input", value: f[key], autocomplete: "off", oninput: (e) => onType(key, e.target.value), ...attrs });
  els = {
    summary: h("p", { class: "muted", text: "Loading…" }),
    level: h(
      "select",
      { class: "input", "aria-label": "Level", onchange: (e) => set({ level: e.target.value }) },
      h("option", { value: "", text: "All levels" }),
      LEVELS.map((l) => h("option", { value: l, selected: f.level === l, text: `${l} and above` })),
    ),
    target: input("target", { list: "log-targets", placeholder: "Module", "aria-label": "Module" }),
    targets: h("datalist", { id: "log-targets" }),
    q: input("q", { type: "search", class: "input grow", placeholder: "Search", "aria-label": "Search" }),
    pause: h("button", { type: "button", class: "btn", onclick: togglePause }, "Pause"),
    status: h("span", { class: "muted small", role: "status" }),
    list: h("ol", { class: "log", "aria-label": "Log lines" }),
    levels: h("section", { class: "card" }),
  };
  el.append(
    h("div", { class: "page-head" }, h("div", {}, h("h1", { text: "Logs" }), els.summary)),
    h(
      "div",
      { class: "stack" },
      h("div", { class: "toolbar", role: "group", "aria-label": "Filters" }, els.level, els.target, els.targets, els.q),
      h(
        "section",
        { class: "card wide" },
        h(
          "div",
          { class: "card-tools" },
          h("div", { class: "toolbar" }, els.pause, els.status),
          h(
            "div",
            { class: "toolbar" },
            h("button", { type: "button", class: "btn btn-quiet", onclick: copy }, "Copy"),
            h("button", { type: "button", class: "btn btn-quiet", onclick: download }, "Download"),
            h("button", { type: "button", class: "btn btn-quiet", onclick: clearView }, "Clear view"),
          ),
        ),
        els.list,
      ),
      els.levels,
    ),
  );
  start();
  loadLevels();
}

export function unmount() {
  stop();
  clearTimeout(typing);
  els = null;
}

// ---- Filters ----------------------------------------------------------------

function onType(key, value) {
  clearTimeout(typing);
  typing = setTimeout(() => set({ [key]: value.trim() }), DEBOUNCE_MS);
}

function set(changes) {
  Object.assign(f, changes);
  const q = new URLSearchParams();
  for (const k of KEYS) if (f[k]) q.set(k, f[k]);
  const qs = q.toString();
  history.replaceState(null, "", `#/logs${qs ? `?${qs}` : ""}`);
  start();
}

const query = (extra) => {
  const q = new URLSearchParams(extra);
  for (const k of KEYS) if (f[k]) q.set(k, f[k]);
  return q;
};

// ---- Lines and the tail -----------------------------------------------------

async function start() {
  stop();
  shown = [];
  waiting = [];
  fill(els.list);
  const run = (tail = { abort: new AbortController() });
  try {
    const data = await get(`/logs?${query({ limit: 500 })}`);
    if (run !== tail) return;
    last = data.last_seq;
    els.summary.textContent = `Keeping the last ${data.capacity.toLocaleString()} lines in memory.`;
    fill(els.targets, data.targets.map((t) => h("option", { value: t })));
    add(data.lines);
    follow(run);
  } catch (e) {
    if (run === tail) els.summary.textContent = `Unavailable: ${e.message}`;
  }
}

function stop() {
  clearTimeout(retry);
  if (tail) tail.abort.abort();
  tail = null;
}

async function follow(run) {
  const headers = { Accept: "text/event-stream" };
  const token = getToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  try {
    const res = await fetch(`/logs/stream?${query({ after: last })}`, { headers, signal: run.abort.signal });
    if (res.status === 401) return get("/logs/level").catch(() => {}); // signs out like any request
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    if (!paused) els.status.textContent = "Live";
    const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
    let buf = "";
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      buf += value;
      let end;
      while ((end = buf.indexOf("\n\n")) >= 0) {
        event(buf.slice(0, end));
        buf = buf.slice(end + 2);
      }
    }
  } catch (e) {
    if (run.abort.signal.aborted) return;
  }
  if (run !== tail || !els) return;
  els.status.textContent = "Disconnected. Reconnecting…";
  retry = setTimeout(() => run === tail && follow(run), RETRY_MS);
}

function event(block) {
  let type = "message";
  let data = "";
  for (const line of block.split("\n")) {
    if (line.startsWith("event:")) type = line.slice(6).trim();
    else if (line.startsWith("data:")) data += line.slice(5).trimStart();
  }
  if (!data) return;
  const msg = JSON.parse(data);
  if (type === "skipped") add([{ skipped: msg.count }]);
  else if (type === "line") {
    last = Math.max(last, msg.seq);
    if (paused) {
      waiting.push(msg);
      els.pause.textContent = `Resume (${waiting.length} waiting)`;
    } else add([msg]);
  }
}

function togglePause() {
  paused = !paused;
  els.status.textContent = paused ? "Paused" : "Live";
  els.pause.textContent = "Pause";
  if (!paused) {
    add(waiting);
    waiting = [];
  }
}

function add(lines) {
  if (!lines.length) return;
  const list = els.list;
  const atEnd = list.scrollTop + list.clientHeight >= list.scrollHeight - 8;
  for (const l of lines) {
    shown.push(l);
    list.append(row(l));
  }
  while (shown.length > MAX_SHOWN) {
    shown.shift();
    list.firstElementChild.remove();
  }
  if (atEnd) list.scrollTop = list.scrollHeight;
}

function value(key, v) {
  const link = LINKS[key];
  if (!link || !link[0].test(v)) return v;
  const href = link[1](v);
  return h("a", href.startsWith("http") ? { href, target: "_blank", rel: "noopener", text: v } : { href, text: v });
}

function row(l) {
  if (l.skipped) return h("li", { class: "log-skip", text: `${l.skipped.toLocaleString()} lines skipped: they left memory before they could be sent.` });
  const t = new Date(l.time);
  return h(
    "li",
    { class: "log-line" },
    h("time", { datetime: l.time, title: dateTime(l.time), text: Number.isNaN(t.getTime()) ? l.time : t.toLocaleTimeString(undefined, { hour12: false }) }),
    h("span", { class: `badge ${BADGE[l.level] || ""}`, text: l.level }),
    h("span", { class: "muted", text: l.target }),
    h("span", { class: "log-msg" }, l.message, l.truncated ? " …" : null, l.fields.map(([k, v]) => [" ", h("span", { class: "muted", text: `${k}=` }), value(k, v)])),
  );
}

// ---- Copy, download, clear --------------------------------------------------

const text = () =>
  shown
    .map((l) => (l.skipped ? `[${l.skipped} lines skipped]` : [l.time, l.level.padEnd(5), `${l.target}:`, l.message, ...l.fields.map(([k, v]) => `${k}=${v}`)].join(" ")))
    .join("\n");

async function copy() {
  try {
    await navigator.clipboard.writeText(text());
    toast(`Copied ${shown.length} lines`);
  } catch (e) {
    toast("Couldn't copy", "error", String(e.message || e));
  }
}

function download() {
  const stamp = new Date().toISOString().replace(/[-:]/g, "").replace("T", "-").slice(0, 15);
  const a = h("a", { href: URL.createObjectURL(new Blob([`${text()}\n`], { type: "text/plain" })), download: `putio-sync-logs-${stamp}.txt` });
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

function clearView() {
  shown = [];
  waiting = [];
  if (paused) els.pause.textContent = "Resume";
  fill(els.list);
}

// ---- Level ------------------------------------------------------------------

async function loadLevels(req) {
  try {
    const s = await (req || get("/logs/level"));
    if (els) renderLevels(s);
  } catch (e) {
    if (req) toastError(e);
    else if (els) fill(els.levels, h("p", { class: "muted", text: `Level unavailable: ${e.message}` }));
  }
}

function renderLevels(s) {
  const target = h("input", { class: "input", list: "log-targets", placeholder: "Everything", "aria-label": "Module", autocomplete: "off" });
  const level = h("select", { class: "input", "aria-label": "Level" }, ["off", ...LEVELS].map((l) => h("option", { value: l, selected: l === "debug", text: l })));
  const keep = h("select", { class: "input", "aria-label": "For how long" }, h("option", { value: "30", text: "30 minutes" }), h("option", { value: "", text: "Keep (until restart)" }));
  const apply = (e) => {
    e.preventDefault();
    const body = { level: level.value, minutes: keep.value ? Number(keep.value) : null };
    if (target.value.trim()) body.target = target.value.trim();
    loadLevels(put("/logs/level", body));
  };
  const revert = (t) => loadLevels(del(`/logs/level${t == null ? "" : `?target=${encodeURIComponent(t)}`}`));
  const overrides = [...(s.global ? [{ ...s.global, target: "" }] : []), ...s.modules];
  fill(
    els.levels,
    h("h2", { text: "Log level" }),
    h("p", {}, "In effect: ", h("code", { text: s.effective }), h("span", { class: "muted", text: ` (base from ${s.source})` })),
    s.available
      ? h(
          "form",
          { class: "toolbar", onsubmit: apply },
          target,
          level,
          keep,
          h("button", { class: "btn btn-primary" }, "Apply"),
        )
      : h("p", { class: "muted", text: "This process can't change its level." }),
    overrides.length
      ? h(
          "ul",
          { class: "log-overrides" },
          overrides.map((o) =>
            h(
              "li",
              {},
              h("code", { text: `${o.target || "everything"}=${o.level}` }),
              h("span", { class: "muted", text: o.until ? ` until ${dateTime(o.until)}` : " until restart" }),
              h("button", { type: "button", class: "btn btn-quiet", onclick: () => revert(o.target) }, "Revert"),
            ),
          ),
          h("li", {}, h("button", { type: "button", class: "btn btn-quiet", onclick: () => revert(null) }, "Revert all")),
        )
      : null,
    h("p", { class: "muted small", text: "Changes here are not saved to the config file. Debug and trace lines are kept only while turned on." }),
  );
}
