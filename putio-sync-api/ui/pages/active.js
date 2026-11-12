// Active page (feature 039): every transfer on its way to the NAS, from Put.io's queue
// to the last file written, once each. Counts by state for each stage filter the list
// (client-side, with a name filter). Polled every 5 s; Put.io figures are at most 10 s
// old, and marked out of date while Put.io isn't answering. Rows can be paused and
// resumed one at a time or in bulk, and the Pause local downloads switch is here too. The
// page adds no retry, cancel or remove of its own: each row links to the page that has them.
// Feature 038: past the NAS an item is "processing"; a row shows the step it is at, and
// opens a timeline of its stages and steps, with Retry and Skip on a failed step.

import { get, post } from "../api.js";
import { confirm, empty, fill, h, nasBanner, poll, setNasPaused, toast } from "../app.js";
import { bytes, dateTime, duration, percent, relative, speed, utc } from "../format.js";
import { selection } from "../select.js";
import { stepLine } from "../timeline.js";
import { timelineView } from "../timeline_view.js";

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
  processing: [
    ["running", "Running"],
    ["held", "Held"],
    ["waiting_for_operator", "Needs you"],
  ],
};
export const STAGES = { putio: "Put.io", nas: "Local", processing: "Processing" };
export const LABEL = Object.fromEntries(Object.values(STATES).flat());
export const BADGE = { downloading: "active", finishing: "active", stalled: "warn", waiting_for_sync: "warn", paused: "warn", error: "bad", failed: "bad", running: "active", held: "warn", waiting_for_operator: "bad" };
const ON_HOLD = "on_hold";

let timer = null;
let seq = 0;
let els = null;
let data = null;
let shown = null;
let shownCounts = null;
// The chip filter: "<stage>:<state>", ON_HOLD, or null for all
let filter = null;
let sel = null;
let banner = null;
// Rows whose timeline is open, by key, and the timelines last fetched for them
let open = new Set();
let timelines = new Map();

// What each pause or resume result reads like in the toast
const RESULT = { paused: "paused", held: "held (Put.io keeps downloading)", resumed: "resumed", already: "already so", not_found: "not found" };

export function mount(el) {
  data = null;
  shown = null;
  shownCounts = null;
  filter = null;
  open = new Set();
  timelines = new Map();
  // Built once: a poll redraws only the counts, the heading and the rows, so the name
  // filter keeps its focus while someone types
  sel = selection({
    actions: [
      h("button", { type: "button", class: "btn btn-small", onclick: () => bulk("pause") }, "Pause"),
      h("button", { type: "button", class: "btn btn-small", onclick: () => bulk("resume") }, "Resume"),
    ],
  });
  banner = nasBanner(() => timer && timer.now());
  els = {
    switch: h("button", { type: "button", class: "btn", onclick: onSwitch }, "Pause local downloads"),
    stale: h("p", { class: "warn-text", hidden: true }),
    counts: h("div", { class: "stack" }),
    title: h("h2", { text: "Transfers" }),
    name: h("input", { type: "search", class: "input grow", placeholder: "Filter by name", "aria-label": "Filter by name", autocomplete: "off", oninput: () => draw(true) }),
    body: h("div", {}, h("p", { class: "muted", text: "Loading…" })),
    finished: h("section", { class: "card wide", hidden: true }),
  };
  el.append(
    h("div", { class: "page-head" }, h("h1", { text: "Active" }), h("div", { class: "toolbar" }, els.switch)),
    h(
      "div",
      { class: "stack" },
      banner.el,
      els.stale,
      h("section", { class: "card wide" }, h("h2", { text: "By state" }), els.counts),
      h("section", { class: "card wide" }, els.title, h("div", { class: "toolbar spaced" }, els.name), els.body),
      els.finished,
    ),
  );
  timer = poll(load, REFRESH_MS);
}

export function unmount() {
  if (timer) timer.stop();
  timer = null;
  els = null;
  data = null;
  sel = null;
  banner = null;
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
  await loadTimelines(mine);
  if (mine !== seq || !els) return;
  draw(false);
}

/** Fetch the timeline of every open row, so a running step shows its progress. */
async function loadTimelines(mine) {
  await Promise.all(
    [...open].map(async (key) => {
      try {
        const t = await get(`/active/${encodeURIComponent(key)}/timeline`);
        if (mine === seq) timelines.set(key, t);
      } catch (e) {
        if (e.status === 404) timelines.set(key, null);
      }
    }),
  );
}

/** Open or close the timeline of `key`. */
async function toggle(key) {
  if (open.has(key)) open.delete(key);
  else open.add(key);
  if (open.has(key)) await loadTimelines(seq);
  draw(true);
}

// ---- Rendering --------------------------------------------------------------

/** Redraw; `force` after a filter change, else only when the response changed. */
function draw(force) {
  if (!els || !data) return;
  const text = JSON.stringify([data, [...timelines]]);
  if (!force && shown === text) return;
  shown = text;

  els.stale.hidden = !data.putio_stale;
  els.stale.textContent = data.putio_stale ? `Put.io isn't answering. Its figures are from ${relative(data.putio_as_of)}; local figures are current.` : "";
  const counts = JSON.stringify([data.counts, (data.items || []).length, filter]);
  if (shownCounts !== counts) {
    shownCounts = counts;
    drawCounts(data.counts, data.items || []);
  }

  banner.update(Boolean(data.nas_paused));
  els.switch.textContent = data.nas_paused ? "Resume local downloads" : "Pause local downloads";
  drawFinished(data.recently_finished || []);

  const items = data.items || [];
  if (items.length === 0) {
    els.title.textContent = "Transfers";
    sel.reset([]);
    els.body.replaceChildren(
      empty("Nothing is downloading right now", "Transfers you add on Put.io show up here until their files are downloaded."),
      h("p", { class: "spaced" }, h("a", { href: "#/transfers" }, "Go to Transfers")),
    );
    return;
  }
  const query = els.name.value.trim().toLowerCase();
  const rows = items.filter((i) => matches(i) && (!query || i.name.toLowerCase().includes(query)));
  els.title.textContent = rows.length === items.length ? `Transfers (${items.length})` : `Transfers (${rows.length} of ${items.length})`;
  // The selection survives a poll for rows still listed and shown
  sel.reset(rows.filter(actionable).map((i) => i.key));
  // `fill` flattens the array; the DOM's replaceChildren would show it as text
  fill(
    els.body,
    rows.length === 0
      ? empty("Nothing matches the filter", "Clear the name filter or choose Show all.")
      : [
          sel.bar,
          h(
            "table",
            { class: "table" },
            h(
              "thead",
              {},
              h(
                "tr",
                {},
                h("th", { class: "select-cell" }, h("span", { class: "visually-hidden", text: "Select" })),
                h("th", { text: "Name" }),
                h("th", { text: "Stage" }),
                h("th", { text: "Progress" }),
                h("th", { class: "right", text: "Size" }),
                h("th", { class: "right", text: "Speed" }),
                h("th", { class: "right", text: "Time left" }),
                h("th", { class: "right", text: "Actions" }),
              ),
            ),
            h("tbody", {}, rows.flatMap((i) => [row(i), open.has(i.key) ? timelineRow(i.key, 8) : null])),
          ),
        ],
  );
  sel.update();
}

// ---- Actions ----------------------------------------------------------------

/** Rows that can be paused: not a Put.io error or a failed file (those are retried on Transfers or Queue) */
function actionable(item) {
  return item.stage !== "processing" && item.state !== "error" && item.state !== "failed";
}

async function onSwitch() {
  const on = !(data && data.nas_paused);
  els.switch.disabled = true;
  await setNasPaused(on);
  if (els) els.switch.disabled = false;
  if (timer) timer.now();
}

/** Pause or resume `keys`; one toast lists each result. */
async function act(verb, keys, button) {
  if (!keys.length) return;
  const names = new Map(((data && data.items) || []).map((i) => [i.key, i.name]));
  const page = sel;
  if (button) button.disabled = true;
  page.busy(true);
  try {
    const r = await post(`/active/${verb}`, { keys });
    const results = r.results || [];
    const lines = results.map((x) => `${names.get(x.key) || x.key}: ${RESULT[x.result] || x.result}`);
    const missing = results.some((x) => x.result === "not_found");
    toast(`${verb === "pause" ? "Pause" : "Resume"} sent for ${keys.length === 1 ? "1 item" : `${keys.length} items`}`, missing ? "error" : "ok", lines.join("; "));
    page.clear();
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toast(`Couldn't ${verb}`, "error", e.details ? `${e.error}: ${e.details}` : e.error);
  } finally {
    page.busy(false);
    if (button && button.isConnected) button.disabled = false;
  }
  if (timer) timer.now();
}

const bulk = (verb) => act(verb, sel.ids);

// ---- Recently finished ------------------------------------------------------

const OUTCOME = { done: ["ok", "Done"], failed: ["bad", "Failed"] };

/** Time in a stage: "—" when either end is unknown */
function took(seconds) {
  return seconds == null ? "—" : seconds < 1 ? "<1s" : duration(seconds);
}

function drawFinished(list) {
  els.finished.hidden = false;
  const head = ["Name", "Outcome", "Finished", "Total", "On Put.io", "Waiting", "Local"];
  els.finished.replaceChildren(
    h("h2", { text: list.length ? `Recently finished (${list.length})` : "Recently finished" }),
    list.length === 0
      ? empty("Nothing has finished recently", "Transfers that finish downloading, or fail, stay here for a day.")
      : h(
          "table",
          { class: "table" },
          h("thead", {}, h("tr", {}, head.map((t, n) => h("th", { class: n >= 3 ? "right" : "", text: t })))),
          h(
            "tbody",
            {},
            list.flatMap((f) => {
              const [tone, label] = OUTCOME[f.outcome] || ["", f.outcome];
              const tr = h(
                "tr",
                {},
                h(
                  "td",
                  { class: "name-cell name", "data-label": "Name" },
                  f.name,
                  f.reason ? h("small", { class: f.outcome === "failed" ? "error-text block" : "muted block", text: f.reason }) : null,
                  f.warnings ? h("button", { type: "button", class: "btn btn-small btn-quiet", "aria-expanded": String(open.has(f.key)), onclick: () => toggle(f.key) }, open.has(f.key) ? "Hide steps" : "Show steps") : null,
                ),
                h("td", { "data-label": "Outcome" }, h("span", { class: `badge ${tone}`, text: label }), f.warnings ? h("span", { class: "badge warn", text: "With warnings" }) : null),
                h("td", { "data-label": "Finished", title: f.done_at ? dateTime(utc(f.done_at)) : "", text: f.done_at ? relative(f.done_at) : "—" }),
                h("td", { class: "num right", "data-label": "Total", text: took(f.total_seconds) }),
                h("td", { class: "num right", "data-label": "On Put.io", text: took(f.putio_seconds) }),
                h("td", { class: "num right", "data-label": "Waiting", text: took(f.waiting_seconds) }),
                h("td", { class: "num right", "data-label": "Local", text: took(f.nas_seconds) }),
              );
              return [tr, f.warnings && open.has(f.key) ? timelineRow(f.key, 7) : null];
            }),
          ),
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
    counts.processing ? line("processing") : null,
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
    i.held ? h("small", { class: "muted block", text: "Put.io keeps downloading; local download held" }) : null,
    i.step ? h("small", { class: i.state === "waiting_for_operator" || i.step.state === "failed" ? "error-text block" : "muted block", text: stepLine(i.step, i.state) }) : null,
  ];
  return h(
    "tr",
    {},
    sel.box(i.key, { label: `Select ${i.name}`, disabled: !actionable(i), title: actionable(i) ? null : "Retry it on the Transfers or Queue page" }),
    h("td", { class: "name-cell name", "data-label": "Name" }, i.name, notes),
    h(
      "td",
      { "data-label": "Stage" },
      h(
        "div",
        { class: "badges" },
        h("span", { class: "badge", text: STAGES[i.stage] }),
        // features 040 and 034: every running file reads back from disk
        i.phase === "checking" || i.phase === "verifying"
          ? h("span", { class: "badge active", text: i.phase === "checking" ? "Checking" : "Verifying" })
          : h("span", { class: `badge ${BADGE[i.state] || ""}`, text: LABEL[i.state] || i.state }),
        i.held ? h("span", { class: "badge warn", title: "Put.io keeps downloading; local download held", text: "On hold" }) : null,
      ),
    ),
    h("td", { class: "num", "data-label": "Progress" }, h("div", {}, percent(pct), files ? ` · ${files}` : null, h("div", { class: "bar" }, h("span", { style: `width:${pct}%` })))),
    h("td", { class: "num right", "data-label": "Size", text: bytes(i.size) }),
    h("td", { class: "num right", "data-label": "Speed", text: !i.paused && i.speed_bps ? speed(i.speed_bps) : "—" }),
    h("td", { class: "num right", "data-label": "Time left", text: i.paused && i.stage === "nas" ? "Paused" : i.eta_seconds != null && i.eta_seconds > 0 ? duration(i.eta_seconds) : "—" }),
    h("td", { class: "actions-cell", "data-label": "Actions" }, h("div", { class: "row-actions" }, pauseButton(i), stepsButton(i), links(i))),
  );
}

function stepsButton(i) {
  if (!i.step && i.stage !== "processing") return null;
  const on = open.has(i.key);
  return h("button", { type: "button", class: "btn btn-small", "aria-expanded": String(on), "aria-label": `${on ? "Hide" : "Show"} the steps of ${i.name}`, onclick: () => toggle(i.key) }, on ? "Hide steps" : "Steps");
}

function pauseButton(i) {
  if (!actionable(i)) return null;
  const verb = i.paused ? "resume" : "pause";
  return h(
    "button",
    { type: "button", class: "btn btn-small", "aria-label": `${i.paused ? "Resume" : "Pause"} ${i.name}`, onclick: (ev) => act(verb, [i.key], ev.currentTarget) },
    i.paused ? "Resume" : "Pause",
  );
}

/** Where to act on the row: Transfers for a Put.io transfer, Queue for its files on the NAS */
function links(i) {
  const l = i.links || {};
  const files = i.stage === "nas" && i.state !== "waiting_for_sync";
  return [
    l.transfers ? h("a", { class: "btn btn-small", href: l.transfers, "aria-label": `Open ${i.name} on Transfers` }, "Transfers") : null,
    files && l.queue ? h("a", { class: "btn btn-small", href: l.queue, "aria-label": `Open the files of ${i.name} on Queue` }, "Queue") : null,
  ];
}

// ---- Timeline (feature 038) -------------------------------------------------

/** The expanded row under an item: its stages left to right, steps with attempts. */
function timelineRow(key, span) {
  const t = timelines.get(key);
  const body =
    t === undefined
      ? h("p", { class: "muted", text: "Loading…" })
      : t === null
        ? h("p", { class: "muted", text: "No steps were recorded for this item." })
        : timelineView(t, stepAction);
  return h("tr", { class: "timeline-row" }, h("td", { colspan: String(span) }, body));
}

/** Retry or skip a step; skipping asks first, since the item goes on without it. */
async function stepAction(t, step, verb, button) {
  if (verb === "skip") {
    const ok = await confirm({ title: `Skip ${step.name}?`, text: `${t.name} goes on without this step.`, verb: "Skip step" });
    if (!ok) return;
  }
  button.disabled = true;
  try {
    await post(`/active/${encodeURIComponent(t.key)}/steps/${step.id}/${verb}`, {});
    toast(verb === "retry" ? `Retrying ${step.name}` : `Skipped ${step.name}`, "ok");
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toast(`Couldn't ${verb} ${step.name}`, "error", e.details ? `${e.error}: ${e.details}` : e.error);
  } finally {
    if (button.isConnected) button.disabled = false;
  }
  if (timer) timer.now();
}
