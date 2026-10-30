// History page: downloads and deletions, newest first, filtered by action and name,
// 50 per page, with totals from /history/stats. "Rejected releases" lists what the
// release pre-check turned away, from /releases/reports (feature 042). Loads on filter
// or page change only. Rows can be flagged fake, and rejected rows marked not fake, with
// the flags for the page loaded in one request.

import { get } from "../api.js";
import { empty, h } from "../app.js";
import { flagControl, loadFlags } from "../flags.js";
import { bytes, dateTime, speed, utc } from "../format.js";

const PAGE = 50;
const DEBOUNCE_MS = 300;
const BADGE = { downloaded: "ok", deleted: "warn" };
const REJECTED = "rejected";
const STAGES = { before: "Before Put.io", putio: "On Put.io" };
const REASONS = { no_wanted: "No wanted file", small_wanted: "Wanted file too small", invalid: "Not a valid .torrent", gone: "Transfer gone from Put.io" };

let els = null;
let offset = 0;
let seq = 0;
let typing = null;
let flags = null;

export function mount(el) {
  offset = 0;
  els = {
    action: h(
      "select",
      { class: "input", "aria-label": "Action", onchange: () => reload() },
      h("option", { value: "", text: "All actions" }),
      h("option", { value: "downloaded", text: "Downloaded" }),
      h("option", { value: "deleted", text: "Deleted" }),
      h("option", { value: REJECTED, text: "Rejected releases" }),
    ),
    name: h("input", { type: "search", class: "input grow", placeholder: "Filter by name", "aria-label": "Filter by name", autocomplete: "off", oninput: onType }),
    totals: h("p", { class: "muted" }),
    list: h("section", { class: "card wide" }, h("p", { class: "muted", text: "Loading…" })),
    pager: h("div", { class: "toolbar pager" }),
  };
  el.append(
    h("div", { class: "page-head" }, h("div", {}, h("h1", { text: "History" }), els.totals), h("div", { class: "toolbar" }, els.action, els.name)),
    h("div", { class: "stack" }, els.list, els.pager),
  );
  loadStats();
  refresh();
}

export function unmount() {
  clearTimeout(typing);
  els = null;
  seq += 1;
}

// ---- Data -------------------------------------------------------------------

function onType() {
  clearTimeout(typing);
  typing = setTimeout(reload, DEBOUNCE_MS);
}

function reload() {
  offset = 0;
  refresh();
}

function refresh() {
  load().catch(() => {});
}

async function loadStats() {
  try {
    const s = await get("/history/stats");
    if (!els) return;
    els.totals.textContent = `${s.total_downloads} downloaded (${bytes(s.total_bytes_downloaded)}) · ${s.total_deletes} deleted · average ${speed(s.average_speed_bps)}`;
  } catch {
    // Totals are optional; the list shows its own error
  }
}

async function load() {
  const mine = ++seq;
  if (els.action.value === REJECTED) return loadRejected(mine);
  const q = new URLSearchParams({ limit: PAGE, offset });
  if (els.action.value) q.set("action", els.action.value);
  const name = els.name.value.trim();
  if (name) q.set("name", name);
  let res;
  try {
    res = await get(`/history?${q}`);
  } catch (e) {
    if (mine !== seq || !els) return;
    failed(e);
    throw e;
  }
  if (mine !== seq || !els) return;
  const entries = res.entries || [];
  // The last page emptied under us: step back to the new last page
  if (!entries.length && offset > 0 && res.total > 0) {
    offset = Math.floor((res.total - 1) / PAGE) * PAGE;
    return load();
  }
  flags = await loadFlags(entries.map((e) => ({ history_id: e.id })));
  if (mine !== seq || !els) return;
  render(entries, res.total, Boolean(name || els.action.value));
}

/** Show `e` in the list with a Retry button */
function failed(e) {
  els.pager.replaceChildren();
  els.list.replaceChildren(
    h("h2", { text: "Can't load history" }),
    h("p", { class: "error-text", text: e.details ? `${e.error}: ${e.details}` : e.error }),
    h("button", { type: "button", class: "btn spaced", onclick: refresh }, "Retry"),
  );
}

/** Rejections are few, so all of them load at once and the name filter and pages are local */
async function loadRejected(mine) {
  let res;
  try {
    res = await get("/releases/reports?range=all");
  } catch (e) {
    if (mine !== seq || !els) return;
    failed(e);
    throw e;
  }
  if (mine !== seq || !els) return;
  const name = els.name.value.trim().toLowerCase();
  const rows = res.filter((r) => r.kind === REJECTED && (!name || r.name.toLowerCase().includes(name)));
  if (offset >= rows.length && offset > 0) offset = Math.floor((rows.length - 1) / PAGE) * PAGE;
  const page = rows.slice(offset, offset + PAGE);
  flags = await loadFlags(page.map((r) => ({ report_id: r.id })));
  if (mine !== seq || !els) return;
  renderRejected(page, rows.length, Boolean(name));
}

// ---- Rendering --------------------------------------------------------------

function pager(shown, total) {
  const step = (label, to, disabled) => h("button", { type: "button", class: "btn btn-small", disabled, onclick: () => ((offset = to), refresh()) }, label);
  els.pager.replaceChildren(
    step("Prev", Math.max(0, offset - PAGE), offset === 0),
    h("span", { class: "muted num", text: `${offset + 1}–${offset + shown} of ${total}` }),
    step("Next", offset + PAGE, offset + shown >= total),
  );
}

function render(entries, total, filtered) {
  if (!entries.length) {
    els.pager.replaceChildren();
    els.list.replaceChildren(h("h2", { text: "History" }), filtered ? empty("Nothing matches the filters", "Clear a filter to see more.") : empty("No downloads or deletions yet", "Finished downloads and removals are listed here."));
    return;
  }
  els.list.replaceChildren(
    h("h2", { text: `History (${total})` }),
    h(
      "table",
      { class: "table" },
      h("thead", {}, h("tr", {}, h("th", { text: "Time" }), h("th", { text: "Action" }), h("th", { text: "Name" }), h("th", { class: "right", text: "Size" }), h("th", { class: "right", text: "Speed" }), h("th", { class: "right", text: "Actions" }))),
      h("tbody", {}, entries.map(row)),
    ),
  );
  pager(entries.length, total);
}

function row(e) {
  const action = String(e.action || "").toLowerCase();
  return h(
    "tr",
    {},
    h("td", { class: "num", "data-label": "Time", text: dateTime(utc(e.timestamp)) }),
    h("td", { "data-label": "Action" }, h("span", { class: `badge ${BADGE[action] || ""}`, text: action || "—" })),
    h("td", { class: "name-cell name", "data-label": "Name", text: e.filename }),
    h("td", { class: "num right", "data-label": "Size", text: bytes(e.file_size) }),
    h("td", { class: "num right", "data-label": "Speed", text: e.speed_bps ? speed(e.speed_bps) : "—" }),
    actions(flagControl(flags, { target: { history_id: e.id }, name: e.filename, onChange: refresh })),
  );
}

function renderRejected(rows, total, filtered) {
  if (!rows.length) {
    els.pager.replaceChildren();
    els.list.replaceChildren(h("h2", { text: "Rejected releases" }), filtered ? empty("Nothing matches the filter", "Clear the name filter to see more.") : empty("Nothing rejected yet", "Releases the pre-check turns away are listed here."));
    return;
  }
  els.list.replaceChildren(
    h("h2", { text: `Rejected releases (${total})` }),
    h(
      "table",
      { class: "table" },
      h("thead", {}, h("tr", {}, h("th", { text: "Time" }), h("th", { text: "Checked" }), h("th", { text: "Name" }), h("th", { text: "Reason" }), h("th", { class: "right", text: "Size" }), h("th", { class: "right", text: "Actions" }))),
      h("tbody", {}, rows.map(rejectedRow)),
    ),
  );
  pager(rows.length, total);
}

/** Why it was rejected, with what the check saw */
function why(r) {
  const label = REASONS[r.reason] || r.reason || "Unknown";
  if (r.reason === "no_wanted" && r.types.length) return `${label} (found ${r.types.map((t) => `.${t.ext}`).join(", ")})`;
  if (r.reason === "small_wanted" && r.min_size) return `${label} (${bytes(r.largest_wanted || 0)}, minimum ${bytes(r.min_size)})`;
  return label;
}

function rejectedRow(r) {
  return h(
    "tr",
    {},
    h("td", { class: "num", "data-label": "Time", text: dateTime(utc(r.created_at)) }),
    h("td", { "data-label": "Checked" }, h("span", { class: "badge bad", text: STAGES[r.stage] || r.stage || "—" })),
    h("td", { class: "name-cell name", "data-label": "Name", text: r.name }),
    h("td", { "data-label": "Reason", text: why(r) }),
    h("td", { class: "num right", "data-label": "Size", text: r.total_size ? bytes(r.total_size) : "—" }),
    actions(flagControl(flags, { target: { report_id: r.id }, name: r.name, rejected: true, onChange: refresh })),
  );
}

const actions = (control) => h("td", { class: "actions-cell", "data-label": "Actions" }, h("div", { class: "row-actions" }, control));
