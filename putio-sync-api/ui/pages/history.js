// History page: downloads and deletions, newest first, filtered by action and name,
// 50 per page, with totals from /history/stats. Loads on filter or page change only.

import { get } from "../api.js";
import { h } from "../app.js";
import { bytes, dateTime, speed, utc } from "../format.js";

const PAGE = 50;
const DEBOUNCE_MS = 300;
const BADGE = { downloaded: "ok", deleted: "warn" };

let els = null;
let offset = 0;
let seq = 0;
let typing = null;

export function mount(el) {
  offset = 0;
  els = {
    action: h(
      "select",
      { class: "input", "aria-label": "Action", onchange: () => reload() },
      h("option", { value: "", text: "All actions" }),
      h("option", { value: "downloaded", text: "Downloaded" }),
      h("option", { value: "deleted", text: "Deleted" }),
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
  const q = new URLSearchParams({ limit: PAGE, offset });
  if (els.action.value) q.set("action", els.action.value);
  const name = els.name.value.trim();
  if (name) q.set("name", name);
  let res;
  try {
    res = await get(`/history?${q}`);
  } catch (e) {
    if (mine !== seq || !els) return;
    els.pager.replaceChildren();
    els.list.replaceChildren(
      h("h2", { text: "Can't load history" }),
      h("p", { class: "error-text", text: e.details ? `${e.error}: ${e.details}` : e.error }),
      h("button", { type: "button", class: "btn spaced", onclick: refresh }, "Retry"),
    );
    throw e;
  }
  if (mine !== seq || !els) return;
  const entries = res.entries || [];
  // The last page emptied under us: step back to the new last page
  if (!entries.length && offset > 0 && res.total > 0) {
    offset = Math.floor((res.total - 1) / PAGE) * PAGE;
    return load();
  }
  render(entries, res.total, Boolean(name || els.action.value));
}

// ---- Rendering --------------------------------------------------------------

function render(entries, total, filtered) {
  if (!entries.length) {
    els.pager.replaceChildren();
    els.list.replaceChildren(h("h2", { text: "History" }), h("p", { class: "muted", text: filtered ? "Nothing matches the filters." : "No downloads or deletions yet." }));
    return;
  }
  els.list.replaceChildren(
    h("h2", { text: `History (${total})` }),
    h(
      "table",
      { class: "table" },
      h("thead", {}, h("tr", {}, h("th", { text: "Time" }), h("th", { text: "Action" }), h("th", { text: "Name" }), h("th", { class: "right", text: "Size" }), h("th", { class: "right", text: "Speed" }))),
      h("tbody", {}, entries.map(row)),
    ),
  );
  const step = (label, to, disabled) => h("button", { type: "button", class: "btn btn-small", disabled, onclick: () => ((offset = to), refresh()) }, label);
  els.pager.replaceChildren(
    step("Prev", Math.max(0, offset - PAGE), offset === 0),
    h("span", { class: "muted num", text: `${offset + 1}–${offset + entries.length} of ${total}` }),
    step("Next", offset + PAGE, offset + entries.length >= total),
  );
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
  );
}
