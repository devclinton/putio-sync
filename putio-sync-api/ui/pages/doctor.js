// Doctor page: the latest health report grouped by area, Run now, and fixes. A fix that
// deletes previews first and asks; the others apply to what the preview lists.

import { get, post } from "../api.js";
import { confirm, h, poll, toast, toastError } from "../app.js";
import { dateTime, relative } from "../format.js";

const REFRESH_MS = 60000;
const CONFIRM_ITEMS = 20;
const AREAS = [
  ["putio", "Put.io"],
  ["sync", "Sync"],
  ["local", "This machine"],
  ["connectivity", "Connection"],
];
const STATUS = {
  ok: ["OK", "ok"],
  warning: ["Warning", "warn"],
  problem: ["Problem", "bad"],
  skipped: ["Skipped", ""],
};

let els = null;
let busy = false;
let poller = null;

export function mount(el) {
  busy = false;
  els = {
    summary: h("p", { class: "muted", text: "Loading…" }),
    run: h("button", { type: "button", class: "btn btn-primary", onclick: runNow }, "Run now"),
    body: h("div", { class: "stack doctor" }),
  };
  el.append(
    h("div", { class: "page-head" }, h("div", {}, h("h1", { text: "Doctor" }), els.summary), h("div", { class: "toolbar" }, els.run)),
    els.body,
  );
  poller = poll(load, REFRESH_MS);
}

export function unmount() {
  els = null;
  poller = null;
}

// ---- Data -------------------------------------------------------------------

async function load() {
  if (busy) return;
  let report;
  try {
    report = await get("/doctor");
  } catch (e) {
    // Nothing has run yet: run now
    if (e.status === 404) return runNow();
    if (els) showError(e);
    throw e;
  }
  if (els) render(report);
}

function setBusy(on, text) {
  busy = on;
  if (!els) return;
  els.run.disabled = on;
  for (const b of els.body.querySelectorAll("button")) b.disabled = on;
  if (on && text) els.summary.textContent = text;
}

async function runNow() {
  if (busy || !els) return;
  setBusy(true, "Running the checks…");
  try {
    const report = await post("/doctor/run");
    if (els) render(report);
  } catch (e) {
    toastError(e);
    if (els) showError(e);
  } finally {
    setBusy(false);
  }
}

function showError(e) {
  els.summary.textContent = "";
  els.body.replaceChildren(
    h(
      "section",
      { class: "card" },
      h("h2", { text: "Can't load the doctor report" }),
      h("p", { class: "error-text", text: e.details ? `${e.error}: ${e.details}` : e.error }),
      h("button", { type: "button", class: "btn spaced", onclick: () => poller && poller.now() }, "Retry"),
    ),
  );
}

// ---- Fixes ------------------------------------------------------------------

function plural(n, word) {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

async function applyFix(fix) {
  if (busy) return;
  // The preview checks again, which can take a few seconds
  setBusy(true, `${fix.label}: checking what to fix…`);
  try {
    const path = `/doctor/fixes/${encodeURIComponent(fix.id)}`;
    const preview = await post(`${path}/preview`);
    if (!preview.items.length) {
      toast("Nothing to fix any more.");
      return;
    }
    if (preview.destructive) {
      const shown = preview.items.slice(0, CONFIRM_ITEMS).map((i) => i.label);
      const more = preview.items.length - shown.length;
      const ok = await confirm({
        title: `${fix.label} ${plural(preview.items.length, "item")}?`,
        text: `${shown.join(", ")}${more > 0 ? `, and ${more} more` : ""}. This can't be undone.`,
        verb: fix.label,
      });
      if (!ok) return;
    }
    const outcome = await post(`${path}/apply`, { items: preview.items.map((i) => i.key), confirm: true });
    const text = `${fix.label}: ${outcome.done.length} done, ${outcome.skipped.length} skipped, ${outcome.failed.length} failed`;
    const first = outcome.failed[0] || outcome.skipped[0];
    toast(text, outcome.failed.length ? "error" : "ok", first ? `${first.item}: ${first.error || first.reason}` : null);
  } catch (e) {
    toastError(e);
  } finally {
    setBusy(false);
    if (poller) poller.now();
  }
}

// ---- Rendering --------------------------------------------------------------

function render(report) {
  const c = report.counts;
  els.summary.replaceChildren(
    h("span", { class: `badge ${STATUS[report.overall][1]}`, text: STATUS[report.overall][0] }),
    ` ${plural(c.problem, "problem")} · ${plural(c.warning, "warning")} · ${c.ok} OK · ${c.skipped} skipped · ran `,
    h("time", { datetime: report.started_at, title: dateTime(report.started_at), text: relative(report.started_at) }),
  );
  els.body.replaceChildren(
    ...AREAS.map(([area, title]) => {
      const checks = report.checks.filter((k) => k.area === area);
      return checks.length ? h("section", { class: "card" }, h("h2", { text: title }), checks.map(check)) : null;
    }),
  );
}

function check(k) {
  const [label, cls] = STATUS[k.status] || STATUS.skipped;
  const more = k.item_count - k.items.length;
  return h(
    "div",
    { class: `doctor-check ${k.status}` },
    h("span", { class: `badge ${cls}`, text: label }),
    h(
      "div",
      { class: "doctor-body" },
      h("p", {}, h("strong", { text: k.title }), " ", k.summary),
      k.suggestion ? h("p", { class: "muted", text: k.suggestion }) : null,
      k.items.length
        ? h(
            "details",
            {},
            h("summary", { text: `Show ${plural(k.item_count, "item")}` }),
            h("ul", {}, k.items.map((i) => h("li", { class: "name" }, i.label, i.detail ? h("small", { class: "muted", text: ` · ${i.detail}` }) : null))),
            more > 0 ? h("p", { class: "muted", text: `and ${more} more` }) : null,
          )
        : null,
      k.fixes.length
        ? h(
            "div",
            { class: "toolbar" },
            k.fixes.map((f) => h("button", { type: "button", class: `btn btn-small${f.destructive ? " btn-danger" : ""}`, disabled: busy, onclick: () => applyFix(f) }, f.label)),
          )
        : null,
    ),
  );
}
