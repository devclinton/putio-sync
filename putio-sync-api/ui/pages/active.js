// Active page (feature 039): every transfer on its way to the NAS, from Put.io's queue
// to the last file written, once each. Counts by state for each stage filter the list
// (client-side, with a name filter). Polled every 5 s; Put.io figures are at most 10 s
// old, and marked out of date while Put.io isn't answering.

import { get } from "../api.js";
import { empty, h, poll } from "../app.js";
import { bytes, duration, percent, relative, speed } from "../format.js";

const REFRESH_MS = 5000;

// Count chips in display order, per stage
const STATES = {
  putio: [
    ["queued", "Queued"],
    ["downloading", "Downloading"],
    ["stalled", "Stalled"],
    ["finishing", "Finishing"],
    ["error", "Error"],
  ],
  nas: [
    ["waiting_for_sync", "Waiting for sync"],
    ["queued", "Queued"],
    ["downloading", "Downloading"],
    ["paused", "Paused"],
    ["failed", "Failed"],
  ],
};
export const STAGES = { putio: "Put.io", nas: "NAS" };
export const LABEL = Object.fromEntries(Object.values(STATES).flat());
export const BADGE = { downloading: "active", finishing: "active", stalled: "warn", waiting_for_sync: "warn", paused: "warn", error: "bad", failed: "bad" };
const ON_HOLD = "on_hold";

let timer = null;
let seq = 0;
let els = null;
let data = null;
let shown = null;
let shownCounts = null;
// The chip filter: "<stage>:<state>", ON_HOLD, or null for all
let filter = null;

export function mount(el) {
  data = null;
  shown = null;
  shownCounts = null;
  filter = null;
  // Built once: a poll redraws only the counts, the heading and the rows, so the name
  // filter keeps its focus while someone types
  els = {
    stale: h("p", { class: "warn-text", hidden: true }),
    counts: h("div", { class: "stack" }),
    title: h("h2", { text: "Transfers" }),
    name: h("input", { type: "search", class: "input grow", placeholder: "Filter by name", "aria-label": "Filter by name", autocomplete: "off", oninput: () => draw(true) }),
    body: h("div", {}, h("p", { class: "muted", text: "Loading…" })),
  };
  el.append(
    h("div", { class: "page-head" }, h("h1", { text: "Active" })),
    h(
      "div",
      { class: "stack" },
      els.stale,
      h("section", { class: "card wide" }, h("h2", { text: "By state" }), els.counts),
      h("section", { class: "card wide" }, els.title, h("div", { class: "toolbar spaced" }, els.name), els.body),
    ),
  );
  timer = poll(load, REFRESH_MS);
}

export function unmount() {
  if (timer) timer.stop();
  timer = null;
  els = null;
  data = null;
  seq += 1;
}

// ---- Data -------------------------------------------------------------------

async function load() {
  const mine = ++seq;
  let res;
  try {
    res = await get("/active");
  } catch (e) {
    if (mine !== seq || !els) return;
    if (e.network || e.status === 401 || e.status === 429) throw e;
    shown = null;
    els.title.textContent = "Can't show active transfers";
    els.body.replaceChildren(
      h("p", { class: "error-text", text: e.details ? `${e.error}: ${e.details}` : e.error }),
      h("button", { type: "button", class: "btn spaced", onclick: () => load().catch(() => {}) }, "Retry"),
    );
    return;
  }
  if (mine !== seq || !els) return;
  data = res;
  draw(false);
}

// ---- Rendering --------------------------------------------------------------

/** Redraw; `force` after a filter change, else only when the response changed. */
function draw(force) {
  if (!els || !data) return;
  const text = JSON.stringify(data);
  if (!force && shown === text) return;
  shown = text;

  els.stale.hidden = !data.putio_stale;
  els.stale.textContent = data.putio_stale ? `Put.io isn't answering. Its figures are from ${relative(data.putio_as_of)}; NAS figures are current.` : "";
  const counts = JSON.stringify([data.counts, (data.items || []).length, filter]);
  if (shownCounts !== counts) {
    shownCounts = counts;
    drawCounts(data.counts, data.items || []);
  }

  const items = data.items || [];
  if (items.length === 0) {
    els.title.textContent = "Transfers";
    els.body.replaceChildren(
      empty("Nothing is downloading right now", "Transfers you add on Put.io show up here until their files are on the NAS."),
      h("p", { class: "spaced" }, h("a", { href: "#/transfers" }, "Go to Transfers")),
    );
    return;
  }
  const query = els.name.value.trim().toLowerCase();
  const rows = items.filter((i) => matches(i) && (!query || i.name.toLowerCase().includes(query)));
  els.title.textContent = rows.length === items.length ? `Transfers (${items.length})` : `Transfers (${rows.length} of ${items.length})`;
  els.body.replaceChildren(
    rows.length === 0
      ? empty("Nothing matches the filter", "Clear the name filter or choose Show all.")
      : h(
          "table",
          { class: "table" },
          h("thead", {}, h("tr", {}, h("th", { text: "Name" }), h("th", { text: "Stage" }), h("th", { text: "Progress" }), h("th", { class: "right", text: "Size" }), h("th", { class: "right", text: "Speed" }), h("th", { class: "right", text: "Time left" }))),
          h("tbody", {}, rows.map(row)),
        ),
  );
}

function matches(item) {
  if (!filter) return true;
  if (filter === ON_HOLD) return item.held;
  return `${item.stage}:${item.state}` === filter;
}

function drawCounts(counts, items) {
  if (!counts) {
    els.counts.replaceChildren();
    return;
  }
  const chip = (key, text, n) =>
    h(
      "button",
      { type: "button", class: `btn btn-small chip${n === 0 ? " zero" : ""}`, "aria-pressed": filter === key ? "true" : "false", onclick: () => setFilter(filter === key ? null : key) },
      `${text} `,
      h("span", { class: "num", text: String(n) }),
    );
  const line = (stage) =>
    h(
      "div",
      { class: "chips" },
      h("span", { class: "chips-label", text: STAGES[stage] }),
      STATES[stage].map(([state, text]) => chip(`${stage}:${state}`, text, (counts[stage] && counts[stage][state]) || 0)),
    );
  const held = (counts.putio && counts.putio.on_hold) || 0;
  els.counts.replaceChildren(
    line("putio"),
    line("nas"),
    h(
      "div",
      { class: "chips" },
      held ? chip(ON_HOLD, "On hold", held) : null,
      h("button", { type: "button", class: "btn btn-small btn-quiet", disabled: !filter, onclick: () => setFilter(null) }, `Show all (${items.length})`),
    ),
  );
}

function setFilter(key) {
  filter = key;
  draw(true);
}

/** "arr:radarr" → "Radarr/Sonarr: radarr"; null when Put.io itself or unknown */
function origin(o) {
  if (!o) return null;
  if (o.startsWith("arr:")) return `Radarr/Sonarr: ${o.slice(4)}`;
  return { watch: "Watch folder", manual: "Web UI" }[o] || null;
}

function row(i) {
  const pct = Math.max(0, Math.min(100, Number(i.percent) || 0));
  const from = origin(i.origin);
  const files = i.files && i.files.total > 1 ? `${i.files.done} of ${i.files.total} files` : null;
  const notes = [
    from ? h("small", { class: "muted block", text: from }) : null,
    i.stall ? h("small", { class: "warn-text block", text: `No progress for ${duration(i.stall.seconds)}: ${i.stall.reason}` }) : null,
    i.state === "waiting_for_sync" ? h("small", { class: "muted block", text: i.next_sync_at ? `Done on Put.io; the next sync ${relative(i.next_sync_at)} picks it up` : "Done on Put.io; the next sync picks it up" }) : null,
    i.seeding ? h("small", { class: "muted block", text: "Still seeding on Put.io" }) : null,
    i.reason ? h("small", { class: "error-text block", text: i.reason }) : null,
    i.note ? h("small", { class: "muted block", text: i.note }) : null,
  ];
  return h(
    "tr",
    {},
    h("td", { class: "name-cell name", "data-label": "Name" }, i.name, notes),
    h(
      "td",
      { "data-label": "Stage" },
      h(
        "div",
        { class: "badges" },
        h("span", { class: "badge", text: STAGES[i.stage] }),
        h("span", { class: `badge ${BADGE[i.state] || ""}`, text: LABEL[i.state] || i.state }),
        i.held ? h("span", { class: "badge warn", text: "on hold" }) : null,
      ),
    ),
    h("td", { class: "num", "data-label": "Progress" }, h("div", {}, percent(pct), files ? ` · ${files}` : null, h("div", { class: "bar" }, h("span", { style: `width:${pct}%` })))),
    h("td", { class: "num right", "data-label": "Size", text: bytes(i.size) }),
    h("td", { class: "num right", "data-label": "Speed", text: i.speed_bps ? speed(i.speed_bps) : "—" }),
    h("td", { class: "num right", "data-label": "Time left", text: i.eta_seconds != null && i.eta_seconds > 0 ? duration(i.eta_seconds) : "—" }),
  );
}
