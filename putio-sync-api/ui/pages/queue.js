// Queue page: active downloads and the queue, polled every 3 s, with every queue action
// and "sync now". Reordering, skip, pause and resume are reversible, so only clearing
// completed rows and cancelling a download ask first. Queue rows can be flagged as fake
// (feature 042); flags reload when the rows or a flag change, not on every poll.

import { del, get, post, syncNow } from "../api.js";
import { confirm, empty, h, poll, toast } from "../app.js";
import { flagControl, loadFlags } from "../flags.js";
import { bytes, duration, percent, relative, speed, utc } from "../format.js";

const REFRESH_MS = 3000;
const STATES = ["pending", "downloading", "paused", "failed", "completed"];
const BADGE = { downloading: "active", completed: "ok", paused: "warn", failed: "bad" };
const MOVES = [
  ["move_up", "↑", "Move up"],
  ["move_down", "↓", "Move down"],
  ["move_first", "⤒", "Move to first"],
  ["move_last", "⤓", "Move to last"],
];

let timer = null;
let seq = 0;
let items = [];
let els = null;
let shown = { downloads: null, queue: null, held: null };
let watchingSync = false;
let flags = null;
let flagged = { ids: null, version: 0 };

export function mount(el) {
  els = {
    summary: h("p", { class: "muted" }, "Loading…"),
    sync: h("p", { class: "sync-line", hidden: true }),
    pause: h("button", { type: "button", class: "btn", onclick: () => bulk("pause", "pause the queue") }, "Pause all"),
    resume: h("button", { type: "button", class: "btn", onclick: () => bulk("resume", "resume the queue") }, "Resume all"),
    clear: h("button", { type: "button", class: "btn", onclick: onClear }, "Clear completed"),
    syncBtn: h("button", { type: "button", class: "btn btn-primary", onclick: onSync }, "Sync now"),
    downloads: h("section", { class: "card wide" }),
    queue: h("section", { class: "card wide" }),
    held: h("section", { class: "card wide", hidden: true }),
  };
  shown = { downloads: null, queue: null, held: null };
  watchingSync = false;
  items = [];
  flags = null;
  flagged = { ids: null, version: 0 };
  el.append(
    h(
      "div",
      { class: "page-head" },
      h("div", {}, h("h1", { text: "Queue" }), els.summary, els.sync),
      h("div", { class: "toolbar" }, els.pause, els.resume, els.clear, els.syncBtn),
    ),
    h("div", { class: "stack" }, els.downloads, els.held, els.queue),
  );
  timer = poll(load, REFRESH_MS);
}

export function unmount() {
  if (timer) timer.stop();
  timer = null;
  els = null;
  seq += 1; // drop any response still in flight
}

// ---- Data -------------------------------------------------------------------

async function load() {
  const mine = ++seq;
  const [queue, downloads, status, arr] = await Promise.all([
    get("/local/queue").catch(failure),
    get("/local/downloads").catch(failure),
    watchingSync ? get("/status").catch(() => null) : null,
    get("/arr/status").catch(() => null),
  ]);
  if (mine !== seq || !els) return; // a newer load already rendered, or page left
  for (const r of [queue, downloads]) if (r && r.failed && passUp(r.err)) throw r.err;
  const ids = queue && !queue.failed ? (queue.items || []).map((i) => i.file_id).join(",") : null;
  if (ids && ids !== flagged.ids) {
    const next = await loadFlags(ids.split(",").map((id) => ({ file_id: Number(id) })));
    if (mine !== seq || !els) return;
    flags = next;
    flagged = { ids, version: flagged.version + 1 };
  }
  renderQueue(queue);
  renderDownloads(downloads);
  if (status && status.sync) renderSync(status.sync);
  renderHeld((arr && arr.held) || []);
}

/** Keep a failed section's error so the other section still renders. */
function failure(err) {
  return { failed: true, err };
}

/** Errors the shell handles: offline banner, sign-in page. */
function passUp(err) {
  return err.network || err.status === 401 || err.status === 429;
}

/** Refresh now, outside the poll timer, after an action. */
function refresh() {
  load().catch(() => {});
}

// ---- Actions ----------------------------------------------------------------

/** Run an action with `button` disabled; toast the server's message or the error. */
async function act(button, what, fn) {
  if (button) button.disabled = true;
  try {
    const res = await fn();
    toast((res && res.message) || "Done");
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toast(`Couldn't ${what}`, "error", reason(e));
  }
  if (button && button.isConnected) button.disabled = false;
  refresh();
}

function reason(e) {
  return e.details ? `${e.error}: ${e.details}` : e.error;
}

function bulk(verb, what) {
  return act(verb === "pause" ? els.pause : els.resume, what, () => post(`/local/queue/${verb}`));
}

async function onClear() {
  const n = items.filter((i) => state(i) === "completed").length;
  if (n === 0) {
    toast("No completed items to clear");
    return;
  }
  const ok = await confirm({
    title: "Clear completed",
    text: `Remove ${n} completed ${n === 1 ? "item" : "items"} from the queue?`,
    verb: "Remove",
  });
  if (ok) await act(els && els.clear, "clear completed items", () => post("/local/queue/clear"));
}

async function onCancel(button, d) {
  const ok = await confirm({
    title: "Cancel download",
    text: `Cancel download of ${d.filename}?`,
    verb: "Cancel download",
  });
  if (ok) await act(button, `cancel ${d.filename}`, () => del(`/local/downloads/${d.file_id}`));
}

async function onSync() {
  els.syncBtn.disabled = true;
  try {
    const sync = await syncNow();
    toast("Sync started");
    watchingSync = true;
    renderSync(sync || { in_progress: true });
  } catch (e) {
    if (e.status === 409) {
      toast("Sync already running");
      watchingSync = true;
      renderSync({ in_progress: true });
    } else if (e.status !== 401 && e.status !== 429) {
      toast("Couldn't start a sync", "error", reason(e));
    }
  }
  if (els) els.syncBtn.disabled = false;
  refresh();
}

// ---- Rendering --------------------------------------------------------------

function state(item) {
  return String(item.status || "").toLowerCase();
}

/** Re-render `section` only when `data` changed, keeping keyboard focus on its button. */
function swap(key, data, build) {
  const text = JSON.stringify(data);
  if (shown[key] === text) return;
  shown[key] = text;
  const section = els[key];
  const focused = section.contains(document.activeElement) ? document.activeElement.dataset.key : null;
  section.replaceChildren(...build());
  if (focused) {
    const again = section.querySelector(`[data-key="${CSS.escape(focused)}"]`);
    if (again && !again.disabled) again.focus();
  }
}

function renderSync(sync) {
  const line = els.sync;
  line.hidden = false;
  if (sync.in_progress) {
    line.replaceChildren(h("span", { class: "badge active", text: "Sync in progress" }));
    return;
  }
  watchingSync = false;
  const o = sync.last_outcome;
  if (o && o.result === "success") {
    const parts = [`${o.found} found`, `${o.queued} queued`, `${o.skipped} skipped`];
    if (o.blocked) parts.push(`${o.blocked} blocked`);
    if (o.failed) parts.push(`${o.failed} failed`);
    line.replaceChildren(h("span", { class: "dot ok" }), `Sync finished: ${parts.join(" · ")}`);
  } else if (o && o.result === "failure") {
    line.replaceChildren(h("span", { class: "dot bad" }), h("span", { class: "error-text", text: `Sync failed: ${o.error}` }));
  } else {
    line.hidden = true;
  }
}

function renderQueue(res) {
  if (res && res.failed) {
    items = [];
    els.summary.replaceChildren(h("span", { class: "error-text", text: `Queue unavailable: ${res.err.error}` }));
    setToolbar({});
    swap("queue", { error: res.err.error }, () => [
      h("h2", { text: "Queue" }),
      h("p", { class: "error-text", text: res.err.error }),
    ]);
    return;
  }
  items = (res && res.items) || [];
  const counts = {};
  for (const i of items) counts[state(i)] = (counts[state(i)] || 0) + 1;
  const parts = STATES.filter((s) => counts[s]).map((s) => `${counts[s]} ${s}`);
  els.summary.textContent = parts.length ? parts.join(" · ") : "The queue is empty";
  setToolbar(counts);

  // The minute keeps the "Added" ages fresh when nothing else changes
  swap("queue", { items, minute: Math.floor(Date.now() / 60000), flags: flagged.version }, () => {
    if (items.length === 0) {
      return [h("h2", { text: "Queue" }), empty("Nothing is queued", "New files in watched Put.io folders are added on the next sync.")];
    }
    const last = items.length - 1;
    const rows = items.map((item, index) => queueRow(item, index, last));
    return [
      h("h2", { text: `Queue (${items.length})` }),
      h(
        "table",
        { class: "table" },
        h(
          "thead",
          {},
          h(
            "tr",
            {},
            h("th", { text: "Name" }),
            h("th", { class: "right", text: "Size" }),
            h("th", { text: "Path" }),
            h("th", { text: "State" }),
            h("th", { class: "right", text: "Priority" }),
            h("th", { text: "Added" }),
            h("th", { class: "right", text: "Actions" }),
          ),
        ),
        h("tbody", {}, rows),
      ),
    ];
  });
}

function setToolbar(counts) {
  els.pause.disabled = !(counts.pending || counts.downloading);
  els.resume.disabled = !counts.paused;
  els.clear.disabled = !counts.completed;
}

function queueRow(item, index, last) {
  const s = state(item);
  const id = item.file_id;
  const blocked = { move_up: index === 0, move_first: index === 0, move_down: index === last, move_last: index === last };
  const buttons = MOVES.map(([verb, glyph, label]) =>
    h(
      "button",
      {
        type: "button",
        class: "btn btn-icon",
        title: label,
        "aria-label": `${label}: ${item.filename}`,
        dataset: { key: `${id}:${verb}` },
        disabled: blocked[verb],
        onclick: (ev) => act(ev.currentTarget, `${label.toLowerCase()} ${item.filename}`, () => post(`/local/queue/${id}/${verb}`)),
      },
      glyph,
    ),
  );
  buttons.push(
    h(
      "button",
      {
        type: "button",
        class: "btn btn-small",
        "aria-label": `Skip ${item.filename}`,
        dataset: { key: `${id}:skip` },
        onclick: (ev) => act(ev.currentTarget, `skip ${item.filename}`, () => post(`/local/queue/${id}/skip`)),
      },
      "Skip",
    ),
  );
  buttons.unshift(flagControl(flags, { target: { file_id: id }, name: item.filename, onChange: reflag }));
  return h(
    "tr",
    {},
    h("td", { class: "name-cell name", "data-label": "Name", text: item.filename }),
    h("td", { class: "num right", "data-label": "Size", text: bytes(item.file_size) }),
    h("td", { class: "name muted", "data-label": "Path", text: item.relative_path || "—" }),
    h("td", { "data-label": "State" }, h("span", { class: `badge ${BADGE[s] || ""}`, text: s || "—" })),
    h("td", { class: "num right", "data-label": "Priority", text: String(item.priority) }),
    h("td", { "data-label": "Added", title: item.created_at, text: relative(utc(item.created_at)) }),
    h("td", { class: "actions-cell", "data-label": "Actions" }, h("div", { class: "row-actions" }, buttons)),
  );
}

/** After a flag changes: refetch the flags on the next load */
function reflag() {
  flagged.ids = null;
  load().catch(() => {});
}

/** Magnets held until a cache has their file list (043) */
function renderHeld(list) {
  els.held.hidden = !list.length;
  swap("held", { list, minute: Math.floor(Date.now() / 60000) }, () =>
    list.length
      ? [
          h("h2", { text: `Waiting for a file list (${list.length})` }),
          h(
            "table",
            { class: "table" },
            h("thead", {}, h("tr", {}, ["Name", "Left", "Asks", "Held"].map((t) => h("th", { text: t })))),
            h(
              "tbody",
              {},
              list.map((m) =>
                h(
                  "tr",
                  {},
                  h("td", { class: "name-cell name", "data-label": "Name", text: m.name }),
                  h("td", { "data-label": "Left", text: `${Math.max(0, Math.ceil((Date.parse(m.until) - Date.now()) / 60000))} min` }),
                  h("td", { class: "num", "data-label": "Asks", text: String(m.attempts) }),
                  h("td", { "data-label": "Held", text: relative(m.since) }),
                ),
              ),
            ),
          ),
        ]
      : [],
  );
}

function renderDownloads(res) {
  if (res && res.failed) {
    swap("downloads", { error: res.err.error }, () => [
      h("h2", { text: "Active downloads" }),
      h("p", { class: "error-text", text: res.err.error }),
    ]);
    return;
  }
  const list = (res && res.downloads) || [];
  // Bytes and speed change every poll; the table is rebuilt, focus is kept by `swap`
  swap("downloads", list, () => {
    if (list.length === 0) {
      return [h("h2", { text: "Active downloads" }), empty("Nothing is downloading", "Queued files start here when a download slot is free.")];
    }
    const rows = list.map((d) => {
      const pct = d.total_bytes ? (100 * d.bytes_downloaded) / d.total_bytes : 0;
      return h(
        "tr",
        {},
        h("td", { class: "name-cell name", "data-label": "Name", text: d.filename }),
        h("td", { class: "num", "data-label": "Done" }, `${bytes(d.bytes_downloaded)} of ${bytes(d.total_bytes)}`),
        h(
          "td",
          { class: "num", "data-label": "Progress" },
          h("div", {}, percent(pct), h("div", { class: "bar" }, h("span", { style: `width:${clamp(pct)}%` }))),
        ),
        // features 040 and 034: reading a file back from disk has no speed or ETA
        d.phase === "checking" || d.phase === "verifying"
          ? h("td", { class: "num right", "data-label": "Speed", colspan: 2 }, h("span", { class: "badge active", text: d.phase === "checking" ? "Checking" : "Verifying" }))
          : [
              h("td", { class: "num right", "data-label": "Speed", text: speed(d.speed_bps) }),
              h("td", { class: "num right", "data-label": "ETA", text: d.eta_seconds != null ? duration(d.eta_seconds) : "—" }),
            ],
        h(
          "td",
          { class: "actions-cell", "data-label": "Actions" },
          h(
            "div",
            { class: "row-actions" },
            h(
              "button",
              {
                type: "button",
                class: "btn btn-small",
                "aria-label": `Cancel download of ${d.filename}`,
                dataset: { key: `${d.file_id}:cancel` },
                onclick: (ev) => onCancel(ev.currentTarget, d),
              },
              "Cancel",
            ),
          ),
        ),
      );
    });
    return [
      h("h2", { text: `Active downloads (${list.length})` }),
      h(
        "table",
        { class: "table" },
        h(
          "thead",
          {},
          h(
            "tr",
            {},
            h("th", { text: "Name" }),
            h("th", { text: "Done" }),
            h("th", { text: "Progress" }),
            h("th", { class: "right", text: "Speed" }),
            h("th", { class: "right", text: "ETA" }),
            h("th", { class: "right", text: "Actions" }),
          ),
        ),
        h("tbody", {}, rows),
      ),
    ];
  });
}

function clamp(x) {
  const n = Number(x);
  if (!Number.isFinite(n)) return 0;
  return Math.max(0, Math.min(100, n)).toFixed(1);
}
