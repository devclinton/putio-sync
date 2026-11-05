// Put.io storage card (feature 033), at the top of the Put.io page: used / free / total
// with a bar, the biggest top-level items, a low-space warning, and "Free up space":
// a preview of the oldest downloaded files to delete (shown in the card), a confirm with
// the count and size, then the run's deleted / skipped / failed. Below, the recent runs
// with their files. `storageNotice()` is the Status page's one-line warning.

import { get, post } from "./api.js";
import { confirm, fill, h, toast } from "./app.js";
import { bytes, dateTime, percent, utc } from "./format.js";

const BASE = "/putio/storage";
const SHOWN = 200;

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
const reason = (e) => (e && (e.error || e.message)) || String(e);
const clamp = (x) => Math.max(0, Math.min(100, Number(x) || 0));
const tone = (s) => (s.free <= s.size * 0.02 ? "bad" : s.low ? "warn" : "");

/** The card; `onFreed` runs after a run deleted something (the listing reloads) */
export function storageCard(onFreed = () => {}) {
  const els = {
    body: h("div", {}, h("p", { class: "muted", text: "Loading…" })),
    plan: h("div", { hidden: true }),
    runs: h("div", {}),
  };
  const el = h("section", { class: "card storage" }, h("h2", { text: "Storage" }), els.body, els.plan, els.runs);
  let summary = null;

  async function load() {
    try {
      summary = await get(BASE);
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      els.body.replaceChildren(h("p", { class: "error-text", text: `Storage unavailable: ${reason(e)}` }), h("button", { type: "button", class: "btn btn-small", onclick: load }, "Retry"));
      return;
    }
    render();
    loadRuns();
  }

  function render() {
    const s = summary;
    const free = h("button", { type: "button", class: `btn btn-small${s.low ? " btn-primary" : ""}`, onclick: (ev) => onPlan(ev.currentTarget) }, "Free up space");
    fill(
      els.body,
      h("p", { class: "big" }, bytes(s.free), h("span", { class: "unit" }, " free")),
      h("p", { class: "muted" }, `${bytes(s.used)} used of ${bytes(s.size)} · ${percent(s.used_percent)} used`),
      h("div", { class: `bar ${tone(s)}` }, h("span", { style: `width:${clamp(s.used_percent)}%` })),
      s.low ? h("p", { class: "storage-warn", role: "status" }, `Low on space: ${bytes(s.free)} free, below the ${s.warn_free_percent}% warning level.`) : null,
      s.top.length ? h("ul", { class: "storage-top" }, s.top.map(topRow)) : h("p", { class: "muted spaced", text: "Nothing on Put.io yet" }),
      h(
        "div",
        { class: "toolbar spaced" },
        free,
        h("span", { class: "muted small" }, `Deletes the oldest downloaded files until ${s.target_free_percent}% is free${s.auto_free ? ` · daily at ${s.run_at} when below ${s.warn_free_percent}%` : ""}`),
      ),
    );
  }

  function topRow(t) {
    const name = t.is_folder ? h("a", { href: `#/putio/${t.id}`, class: "name", text: t.name }) : h("span", { class: "name", text: t.name });
    return h("li", {}, name, h("span", { class: "muted" }, `${bytes(t.size)} · ${percent(t.share_percent)}`), h("div", { class: "bar" }, h("span", { style: `width:${clamp(t.share_percent)}%` })));
  }

  async function onPlan(button) {
    button.disabled = true;
    button.textContent = "Checking…";
    let plan;
    try {
      plan = await post(`${BASE}/plan`);
    } catch (e) {
      if (e.status !== 401 && e.status !== 429) toast("Couldn't plan a free-up", "error", reason(e));
      return;
    } finally {
      if (button.isConnected) {
        button.disabled = false;
        button.textContent = "Free up space";
      }
    }
    showPlan(plan);
  }

  function closePlan() {
    els.plan.hidden = true;
    els.plan.replaceChildren();
  }

  function showPlan(p) {
    els.plan.hidden = false;
    const close = h("button", { type: "button", class: "btn btn-small", onclick: closePlan }, "Cancel");
    const head = h("h3", { text: "Free up space" });
    if (p.needed_bytes === 0) {
      els.plan.replaceChildren(head, h("p", { class: "muted", text: `${bytes(p.free_before)} is free, above the ${bytes(p.target_bytes)} target. Nothing to delete.` }), h("div", { class: "toolbar" }, close));
      return;
    }
    const notes = [];
    if (p.shortfall_bytes > 0) notes.push(h("p", { class: "storage-warn", text: `Not enough downloaded files to reach the target: ${bytes(p.shortfall_bytes)} short.` }));
    if (!p.complete) notes.push(h("p", { class: "muted", text: "Put.io could not be listed completely; some files were not considered." }));
    if (!p.files.length) {
      els.plan.replaceChildren(head, h("p", { class: "muted", text: "No files are safe to delete: only files downloaded here, with the local copy present, and not queued, shared or part of a transfer are." }), ...notes, h("div", { class: "toolbar" }, close));
      return;
    }
    const rows = p.files.slice(0, SHOWN).map((f) =>
      h("tr", {}, h("td", { class: "name" }, h("div", { text: f.path }), h("div", { class: "muted small", text: f.local_path })), h("td", { text: bytes(f.size) }), h("td", { class: "muted", text: f.created_at ? dateTime(utc(f.created_at)) : "—" })),
    );
    const more = p.files.length > SHOWN ? h("p", { class: "muted", text: `and ${p.files.length - SHOWN} more` }) : null;
    const go = h("button", { type: "button", class: "btn btn-small btn-danger", onclick: (ev) => onFree(ev.currentTarget, p) }, `Delete ${plural(p.files.length, "file", "files")}`);
    els.plan.replaceChildren(
      head,
      h("p", {}, `${plural(p.files.length, "file", "files")}, ${bytes(p.total_bytes)}, oldest first: free goes from ${bytes(p.free_before)} to ${bytes(p.free_after)} (target ${bytes(p.target_bytes)}).`),
      ...notes,
      h("div", { class: "table-wrap" }, h("table", { class: "table" }, h("thead", {}, h("tr", {}, h("th", { text: "Put.io path / local copy" }), h("th", { text: "Size" }), h("th", { text: "Added" }))), h("tbody", {}, rows))),
      more,
      h("div", { class: "toolbar spaced" }, go, close),
    );
  }

  async function onFree(button, p) {
    const ok = await confirm({
      title: "Free up space",
      text: `Delete ${plural(p.files.length, "file", "files")} (${bytes(p.total_bytes)}) from Put.io? Their local copies stay. This cannot be undone.`,
      verb: "Delete",
    });
    if (!ok) return;
    button.disabled = true;
    button.textContent = "Deleting…";
    let r;
    try {
      r = await post(`${BASE}/free`, { plan_id: p.id, file_ids: p.files.map((f) => f.id) });
    } catch (e) {
      if (e.status !== 401 && e.status !== 429) toast("Couldn't free up space", "error", reason(e));
      if (e.status === 409) closePlan(); // expired or another run: preview again
      else if (button.isConnected) {
        button.disabled = false;
        button.textContent = `Delete ${plural(p.files.length, "file", "files")}`;
      }
      return;
    }
    showResult(r);
    if (r.deleted.length) onFreed();
    load();
  }

  function showResult(r) {
    const bad = r.failed.length > 0;
    toast(`Deleted ${plural(r.deleted.length, "file", "files")} (${bytes(r.bytes_deleted)})`, bad ? "error" : "ok", bad ? `${plural(r.failed.length, "file", "files")} failed` : null);
    const list = (title, items, text) => (items.length ? [h("h4", { text: title }), h("ul", { class: "paths" }, items.map((x) => h("li", { text: text(x) })))] : []);
    els.plan.replaceChildren(
      h("h3", { text: "Free up space" }),
      h("p", {}, `Deleted ${plural(r.deleted.length, "file", "files")}, ${bytes(r.bytes_deleted)}${r.free_after != null ? `; ${bytes(r.free_after)} free now` : ""}.`),
      ...list("Skipped", r.skipped, (s) => `${s.path}: ${s.reason}`),
      ...list("Failed", r.failed, (f) => `${f.path}: ${f.error}`),
      h("div", { class: "toolbar spaced" }, h("button", { type: "button", class: "btn btn-small", onclick: closePlan }, "Close")),
    );
  }

  async function loadRuns() {
    let runs;
    try {
      runs = await get(`${BASE}/runs`);
    } catch {
      els.runs.replaceChildren();
      return;
    }
    if (!runs.length) {
      els.runs.replaceChildren();
      return;
    }
    els.runs.replaceChildren(h("h3", { text: "Recent runs" }), h("ul", { class: "storage-runs" }, runs.map(runRow)));
  }

  function runRow(run) {
    const by = run.trigger === "schedule" ? "automatic" : "manual";
    const failed = run.failed ? `, ${run.failed} failed` : "";
    return h(
      "li",
      {},
      h(
        "details",
        {},
        h("summary", {}, `${dateTime(utc(run.finished_at))} · ${by} · ${plural(run.files, "file", "files")}, ${bytes(run.bytes)}${failed}`),
        h("ul", { class: "paths" }, run.deleted.map((f) => h("li", { text: `${f.path} (${bytes(f.size)}) → kept at ${f.local_path}` }))),
      ),
    );
  }

  load();
  return { el, reload: load };
}

/** The Status page's warning when Put.io is low on space; empty otherwise */
export function storageNotice() {
  const el = h("div", { hidden: true });
  get(BASE)
    .then((s) => {
      if (!s.low) return;
      el.hidden = false;
      el.className = "notice storage-warn";
      el.replaceChildren(`Put.io is low on space: ${bytes(s.free)} free of ${bytes(s.size)}, below the ${s.warn_free_percent}% warning level. `, h("a", { href: "#/putio", text: "Free up space" }));
    })
    .catch(() => {});
  return el;
}
