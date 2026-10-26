// Status page: one GET /status every 3 s, answering "is it working?".

import { get, syncNow } from "../api.js";
import { empty, h, poll, toast, toastError } from "../app.js";
import { bytes, duration, percent, relative, speed } from "../format.js";

const REFRESH_MS = 3000;

let timer = null;
let syncButton = null;
let summary = null;
let grid = null;

export function mount(el) {
  syncButton = h("button", { type: "button", class: "btn btn-primary", onclick: onSync }, "Sync now");
  summary = h("p", { class: "muted" }, "Loading…");
  grid = h("div", { class: "grid" });
  el.append(
    h("div", { class: "page-head" }, h("div", {}, h("h1", { text: "Status" }), summary), syncButton),
    grid,
  );
  timer = poll(refresh, REFRESH_MS);
}

export function unmount() {
  if (timer) timer.stop();
  timer = null;
}

async function refresh() {
  let data;
  try {
    data = await get("/status");
  } catch (e) {
    if (e.network || e.status === 401 || e.status === 429) throw e;
    summary.replaceChildren(h("span", { class: "error-text", text: `Status unavailable: ${e.error}` }));
    return;
  }
  render(data);
}

async function onSync() {
  syncButton.disabled = true;
  try {
    await syncNow();
    toast("Sync started");
  } catch (e) {
    if (e.status === 409) toast("Sync already running");
    else toastError(e);
  }
  if (timer) timer.now();
}

// ---- Rendering --------------------------------------------------------------

function render(s) {
  const reasons = new Map((s.unavailable || []).map((u) => [u.section, u.reason]));
  const syncing = Boolean(s.sync && s.sync.in_progress);
  syncButton.disabled = syncing;
  syncButton.textContent = syncing ? "Syncing…" : "Sync now";

  summary.replaceChildren(...headline(s));
  grid.replaceChildren(
    syncCard(s.sync),
    queueCard(s.queue, reasons.get("queue")),
    downloadsCard(s.downloads, reasons.get("downloads")),
    workersCard(s.workers),
    diskCard(s.disk, reasons.get("disk")),
    watcherCard(s.watcher),
    connectionsCard(s.reachability, s.auth),
    buildCard(s.build, s.uptime_seconds),
  );
}

/** The one-line answer at the top of the page. */
function headline(s) {
  const r = s.reachability || {};
  let tone = "ok";
  let text = "Running normally";
  if (r.database !== "ok") {
    tone = "bad";
    text = "Database unavailable";
  } else if (r.putio === "failed") {
    tone = "bad";
    text = "Last sync failed";
  } else if (s.sync && s.sync.in_progress) {
    tone = "ok";
    text = "Syncing with Put.io";
  } else if (r.putio === "unknown") {
    tone = "warn";
    text = "Waiting for the first sync";
  }
  return [h("span", { class: `dot ${tone}` }), text];
}

function card(title, ...body) {
  return h("section", { class: "card" }, h("h2", { text: title }), ...body);
}

function unavailableCard(title, reason) {
  return card(title, h("p", { class: "muted", text: `Unavailable: ${reason || "no data"}` }));
}

function kv(rows) {
  return h(
    "dl",
    { class: "kv" },
    rows.filter(Boolean).map(([k, v]) => [h("dt", { text: k }), h("dd", {}, v)]),
  );
}

function syncCard(sync) {
  if (!sync) return unavailableCard("Sync");
  const o = sync.last_outcome;
  let result;
  if (!o) result = h("span", { class: "muted", text: "No sync yet" });
  else if (o.result === "success") result = [h("span", { class: "dot ok" }), "Succeeded"];
  else result = [h("span", { class: "dot bad" }), "Failed"];

  let next;
  if (sync.in_progress) next = h("span", { class: "badge active", text: "In progress" });
  else if (sync.next_due_at) next = relative(sync.next_due_at);
  else next = h("span", { class: "muted", text: "Not scheduled" });

  const rows = [
    ["Last run", sync.last_started_at ? relative(sync.last_started_at) : "—"],
    ["Result", result],
    sync.last_duration_seconds != null ? ["Took", duration(sync.last_duration_seconds)] : null,
    ["Next run", next],
    sync.interval_seconds ? ["Every", duration(sync.interval_seconds)] : null,
  ];
  let detail = null;
  if (o && o.result === "success") {
    const parts = [`${o.found} found`, `${o.queued} queued`, `${o.skipped} skipped`];
    if (o.blocked) parts.push(`${o.blocked} blocked`);
    if (o.failed) parts.push(`${o.failed} failed`);
    detail = h("p", { class: "muted spaced", text: parts.join(" · ") });
  } else if (o && o.result === "failure") {
    detail = h("p", { class: "error-text spaced", text: o.error });
  }
  return card("Sync", kv(rows), detail);
}

function queueCard(q, reason) {
  if (!q) return unavailableCard("Queue", reason);
  const items = [
    ["Pending", q.pending],
    ["Downloading", q.downloading],
    ["Paused", q.paused],
    ["Completed", q.completed],
    ["Failed", q.failed],
  ];
  if (q.skipped) items.push(["Skipped", q.skipped]);
  return card(
    "Queue",
    h(
      "dl",
      { class: "counts" },
      items.map(([k, v]) => h("div", {}, h("dt", { text: k }), h("dd", { text: String(v) }))),
    ),
  );
}

function downloadsCard(list, reason) {
  if (!list) {
    const c = unavailableCard("Active downloads", reason);
    c.classList.add("wide");
    return c;
  }
  if (list.length === 0) {
    return h("section", { class: "card wide" }, h("h2", { text: "Active downloads" }), empty("Nothing is downloading", "Queued files start here when a download slot is free."));
  }
  const rows = list.map((d) =>
    h(
      "tr",
      {},
      h("td", { class: "name-cell name", "data-label": "Name", text: d.filename }),
      h("td", { "data-label": "Done", class: "num" }, `${bytes(d.bytes_downloaded)} of ${bytes(d.file_size)}`),
      h(
        "td",
        { "data-label": "Progress", class: "num" },
        h("div", {}, percent(d.percentage), h("div", { class: "bar" }, h("span", { style: `width:${clamp(d.percentage)}%` }))),
      ),
      h("td", { "data-label": "Speed", class: "num right", text: speed(d.speed_bps) }),
      h("td", { "data-label": "ETA", class: "num right", text: d.eta_seconds != null ? duration(d.eta_seconds) : "—" }),
    ),
  );
  return h(
    "section",
    { class: "card wide" },
    h("h2", { text: "Active downloads" }),
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
        ),
      ),
      h("tbody", {}, rows),
    ),
  );
}

function workersCard(w) {
  if (!w) return unavailableCard("Workers");
  const pct = w.configured ? (100 * w.busy) / w.configured : 0;
  return card(
    "Workers",
    h("p", { class: "big" }, `${w.busy}`, h("span", { class: "unit" }, ` of ${w.configured} busy`)),
    h("div", { class: "bar" }, h("span", { style: `width:${clamp(pct)}%` })),
  );
}

function diskCard(d, reason) {
  if (!d) return unavailableCard("Disk", reason);
  const used = d.total_bytes ? (100 * (d.total_bytes - d.free_bytes)) / d.total_bytes : 0;
  const tone = used >= 95 ? "bad" : used >= 85 ? "warn" : "";
  return card(
    "Disk",
    h("p", { class: "big" }, bytes(d.free_bytes), h("span", { class: "unit" }, " free")),
    h("p", { class: "muted" }, `of ${bytes(d.total_bytes)} · ${percent(used)} used`),
    h("div", { class: `bar ${tone}` }, h("span", { style: `width:${clamp(used)}%` })),
  );
}

function watcherCard(w) {
  if (!w) return unavailableCard("Watcher");
  return card(
    "Watcher",
    kv([
      ["State", w.configured ? [h("span", { class: "dot ok" }), "On"] : [h("span", { class: "dot" }), "Off"]],
      ["Processed", w.files_processed != null ? String(w.files_processed) : "—"],
    ]),
    w.paths && w.paths.length
      ? h("ul", { class: "paths" }, w.paths.map((p) => h("li", { text: p })))
      : h("p", { class: "muted spaced", text: "No folders configured" }),
  );
}

const REACH = {
  ok: ["ok", "OK"],
  failed: ["bad", "Failed"],
  unknown: ["", "Unknown"],
  unavailable: ["bad", "Unavailable"],
};
const PUTIO_AUTH = { token: "OAuth token", app_password: "App password", none: "Not configured" };

function connectionsCard(r, auth) {
  const dot = (state) => {
    const [tone, label] = REACH[state] || ["", state || "—"];
    return [h("span", { class: `dot ${tone}` }), label];
  };
  return card(
    "Connections",
    kv([
      ["Database", dot(r && r.database)],
      ["Put.io", dot(r && r.putio)],
      auth ? ["Put.io sign-in", PUTIO_AUTH[auth.putio] || auth.putio] : null,
    ]),
  );
}

function buildCard(build, uptime) {
  return card(
    "Daemon",
    kv([
      ["Version", build ? build.version : "—"],
      build && build.commit && build.commit !== "unknown" ? ["Commit", build.commit.slice(0, 12)] : null,
      ["Uptime", duration(uptime)],
    ]),
  );
}

function clamp(x) {
  const n = Number(x);
  if (!Number.isFinite(n)) return 0;
  return Math.max(0, Math.min(100, n)).toFixed(1);
}
