// Transfers page: Put.io transfers polled every 5 s, an add form (magnet/URL or a
// .torrent file), retry for a failed transfer, and remove with confirmation. Put.io has
// no pause or resume, so there is no start/stop. Feature 007: a Stalled badge on
// transfers the daemon saw stop making progress, Copy magnet, and Re-add (cancel, then
// add the same magnet again) for stalled ones. Feature 008: tick rows (or Select all) to
// retry or remove several at once, and clear completed, failed or all finished transfers
// (a dry run for the count, a confirm, then one clean on Put.io).

import { del, get, post } from "../api.js";
import { confirm, empty, h, poll, toast } from "../app.js";
import { bytes, duration, percent, speed } from "../format.js";
import { selection } from "../select.js";

const REFRESH_MS = 5000;
const BADGE = { DOWNLOADING: "active", COMPLETED: "ok", SEEDING: "ok", ERROR: "bad", FAILED: "bad", WAITING: "warn", IN_QUEUE: "warn" };
// States Put.io can retry (`POST /transfers/retry`)
const RETRYABLE = new Set(["ERROR", "FAILED"]);

let timer = null;
let seq = 0;
let els = null;
let shown = null;
let sel = null;
let current = [];

// Clear buttons: status group sent to the API, button text, what the toast calls them
const CLEARS = [
  { status: "completed", label: "Clear completed", what: "completed" },
  { status: "failed", label: "Clear failed", what: "failed" },
  { status: "finished", label: "Clear all finished", what: "finished" },
];

export function mount(el) {
  shown = null;
  els = {
    url: h("input", { type: "text", class: "input grow", name: "url", placeholder: "Magnet link or torrent URL", "aria-label": "Magnet link or torrent URL", autocomplete: "off" }),
    file: h("input", { type: "file", class: "input", accept: ".torrent,application/x-bittorrent", "aria-label": "Torrent file" }),
    add: h("button", { type: "submit", class: "btn btn-primary" }, "Add"),
    list: h("section", { class: "card wide" }, h("p", { class: "muted", text: "Loading…" })),
  };
  current = [];
  sel = selection({
    actions: [
      h("button", { type: "button", class: "btn btn-small", onclick: onRetrySelected }, "Retry"),
      h("button", { type: "button", class: "btn btn-small", onclick: onRemoveSelected }, "Remove"),
    ],
  });
  const clears = CLEARS.map((c) => h("button", { type: "button", class: "btn", title: `Remove ${c.what} transfers from the Put.io transfer list; their files stay in your account`, onclick: (ev) => onClear(ev.currentTarget, c) }, c.label));
  el.append(
    h("div", { class: "page-head" }, h("h1", { text: "Transfers" }), h("div", { class: "toolbar" }, ...clears)),
    h(
      "div",
      { class: "stack" },
      h("form", { class: "card add-form", onsubmit: onAdd }, h("h2", { text: "Add a transfer" }), h("div", { class: "toolbar" }, els.url, els.file, els.add)),
      els.list,
    ),
  );
  timer = poll(load, REFRESH_MS);
}

export function unmount() {
  if (timer) timer.stop();
  timer = null;
  els = null;
  sel = null;
  seq += 1;
}

// ---- Data -------------------------------------------------------------------

async function load() {
  const mine = ++seq;
  let res;
  try {
    res = await get("/putio/transfers");
  } catch (e) {
    if (mine !== seq || !els) return;
    if (e.network || e.status === 401 || e.status === 429) throw e;
    // Keep polling; the Retry button refreshes at once
    shown = null;
    els.list.replaceChildren(
      h("h2", { text: "Can't list transfers" }),
      h("p", { class: "error-text", text: reason(e) }),
      h("button", { type: "button", class: "btn spaced", onclick: refresh }, "Retry"),
    );
    return;
  }
  if (mine !== seq || !els) return;
  render(res.items || []);
}

function refresh() {
  load().catch(() => {});
}

function reason(e) {
  return e.details ? `${e.error}: ${e.details}` : e.error;
}

// ---- Actions ----------------------------------------------------------------

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

/** A file's bytes as base64, without the `data:...;base64,` prefix. */
function base64(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).replace(/^data:[^,]*,/, ""));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

async function onAdd(ev) {
  ev.preventDefault();
  const url = els.url.value.trim();
  const file = els.file.files[0];
  if (!url && !file) {
    toast("Paste a magnet link or URL, or choose a .torrent file", "error");
    return;
  }
  els.add.disabled = true;
  try {
    const body = file ? { file_data: await base64(file) } : { url };
    const t = await post("/putio/transfers", body);
    toast(`Added ${(t && t.name) || "transfer"}`);
    els.url.value = "";
    els.file.value = "";
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toast("Couldn't add the transfer", "error", reason(e && e.error ? e : { error: String(e) }));
  }
  if (els) els.add.disabled = false;
  refresh();
}

function onRetry(button, t) {
  return act(button, `retry ${t.name}`, async () => {
    await post(`/putio/transfers/${t.id}/retry`);
    return { message: `Retrying ${t.name}` };
  });
}

/** Copy `text`; falls back to a hidden textarea where the Clipboard API is missing
 * (it needs HTTPS, and the NAS serves plain HTTP) or refused. */
async function copyText(text) {
  if (navigator.clipboard && window.isSecureContext) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // Permission refused: try the hidden textarea
    }
  }
  const area = h("textarea", { readonly: true, style: "position:fixed;top:0;left:0;opacity:0" });
  area.value = text;
  document.body.append(area);
  area.select();
  const ok = document.execCommand("copy");
  area.remove();
  if (!ok) throw new Error("copy refused");
}

async function onCopyMagnet(t) {
  try {
    await copyText(t.magnet);
    toast("Magnet link copied");
  } catch {
    toast("Couldn't copy; here is the magnet link", "error", t.magnet);
  }
}

async function onReadd(button, t) {
  const ok = await confirm({
    title: "Re-add transfer",
    text: `Re-add ${t.name}? Put.io will cancel it and start it again from the same magnet link, usually from 0%.`,
    verb: "Re-add",
  });
  if (!ok) return;
  await act(button, `re-add ${t.name}`, async () => {
    const res = await post(`/putio/transfers/${t.id}/readd`);
    return { message: `Re-added ${(res && res.transfer && res.transfer.name) || t.name}` };
  });
}

async function onRemove(button, t) {
  const ok = await confirm({ title: "Remove transfer", text: `Remove transfer ${t.name}?`, verb: "Remove" });
  if (ok) await act(button, `remove ${t.name}`, () => del(`/putio/transfers/${t.id}`));
}

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
const SHOWN_NAMES = 5;

/** Up to five names, then "and N more". */
function some(names) {
  const more = names.length > SHOWN_NAMES ? `, and ${names.length - SHOWN_NAMES} more` : "";
  return names.slice(0, SHOWN_NAMES).join(", ") + more;
}

const selectedTransfers = () => current.filter((t) => sel.has(t.id));

/** Run a bulk action with the bar disabled; clears the selection and refreshes after. */
async function bulk(what, fn) {
  const page = sel;
  page.busy(true);
  try {
    await fn();
    page.clear();
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toast(`Couldn't ${what}`, "error", reason(e));
  } finally {
    page.busy(false);
  }
  refresh();
}

/** Retry the selected transfers Put.io can retry; one toast with what it refused. */
function onRetrySelected() {
  const picked = selectedTransfers().filter((t) => RETRYABLE.has(String(t.status || "").toUpperCase()));
  if (picked.length === 0) {
    toast("None of the selected transfers has failed, so there is nothing to retry", "error");
    return;
  }
  return bulk("retry the transfers", async () => {
    const r = await post("/putio/transfers/retry", { transfer_ids: picked.map((t) => t.id) });
    const names = new Map(picked.map((t) => [t.id, t.name]));
    const failed = r.failed || [];
    toast(
      `Retrying ${plural((r.retried || []).length, "transfer", "transfers")}`,
      failed.length ? "error" : "ok",
      failed.length ? `Put.io refused ${some(failed.map((f) => `${names.get(f.id) || f.id} (${f.error})`))}` : null,
    );
  });
}

async function onRemoveSelected() {
  const picked = selectedTransfers();
  const n = picked.length;
  const ok = await confirm({ title: "Remove transfers", text: `Remove ${plural(n, "transfer", "transfers")} (${some(picked.map((t) => t.name))})? Put.io stops any that are still running.`, verb: "Remove" });
  if (!ok || !sel) return;
  await bulk("remove the transfers", async () => {
    const r = await post("/putio/transfers/remove", { transfer_ids: picked.map((t) => t.id) });
    toast((r && r.message) || `Removed ${plural(n, "transfer", "transfers")}`);
  });
}

/** Dry run for the count, confirm, then clear; nothing to clear → a toast only. */
async function onClear(button, { status, what }) {
  button.disabled = true;
  try {
    const preview = await post("/putio/transfers/clear", { status, dry_run: true });
    const n = preview.count;
    if (n === 0) {
      toast(`No ${what} transfers to clear`);
      return;
    }
    const ok = await confirm({
      title: "Clear transfers",
      text: `Clear ${plural(n, `${what} transfer`, `${what} transfers`)} (${some(preview.transfers.map((t) => t.name))}) from the Put.io transfer list? Their files stay in your Put.io account.`,
      verb: "Clear",
    });
    if (!ok) return;
    const r = await post("/putio/transfers/clear", { status, dry_run: false });
    toast(`Cleared ${plural(r.count, "transfer", "transfers")}`);
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toast(`Couldn't clear ${what} transfers`, "error", reason(e));
  } finally {
    if (button.isConnected) button.disabled = false;
  }
  refresh();
}

// ---- Rendering --------------------------------------------------------------

function render(list) {
  const text = JSON.stringify(list);
  if (shown === text) return;
  shown = text;
  current = list;
  // The selection survives a refresh for transfers still listed
  sel.reset(list.map((t) => t.id));
  if (list.length === 0) {
    els.list.replaceChildren(h("h2", { text: "Transfers" }), empty("No transfers on Put.io", "Transfers you add on Put.io show up here."));
    sel.update();
    return;
  }
  els.list.replaceChildren(
    h("h2", { text: `Transfers (${list.length})` }),
    sel.bar,
    h(
      "table",
      { class: "table" },
      h(
        "thead",
        {},
        h("tr", {}, h("th", { class: "select-cell" }, h("span", { class: "visually-hidden", text: "Select" })), h("th", { text: "Name" }), h("th", { text: "Status" }), h("th", { text: "Progress" }), h("th", { class: "right", text: "Size" }), h("th", { class: "right", text: "Speed" }), h("th", { class: "right", text: "Actions" })),
      ),
      h("tbody", {}, list.map(row)),
    ),
  );
  sel.update();
}

function row(t) {
  const s = String(t.status || "").toUpperCase();
  const pct = Math.max(0, Math.min(100, Number(t.percent_done) || 0));
  const stall = t.stall;
  return h(
    "tr",
    {},
    sel.box(t.id, { label: `Select ${t.name}` }),
    h(
      "td",
      { class: "name-cell name", "data-label": "Name" },
      t.name,
      t.error_message ? h("small", { class: "error-text block", text: t.error_message }) : null,
      stall ? h("small", { class: "warn-text block", text: `No progress for ${duration(stall.seconds)}: ${stall.reason}` }) : null,
    ),
    h(
      "td",
      { "data-label": "Status" },
      h(
        "div",
        { class: "badges" },
        h("span", { class: `badge ${BADGE[s] || ""}`, text: s.toLowerCase().replace(/_/g, " ") || "—" }),
        stall ? h("span", { class: "badge warn", title: stall.reason, text: `stalled ${duration(stall.seconds)}` }) : null,
      ),
    ),
    h("td", { class: "num", "data-label": "Progress" }, h("div", {}, percent(pct), h("div", { class: "bar" }, h("span", { style: `width:${pct}%` })))),
    h("td", { class: "num right", "data-label": "Size", text: bytes(t.size) }),
    h("td", { class: "num right", "data-label": "Speed", text: t.down_speed ? speed(t.down_speed) : "—" }),
    h(
      "td",
      { class: "actions-cell", "data-label": "Actions" },
      h(
        "div",
        { class: "row-actions" },
        RETRYABLE.has(s) ? h("button", { type: "button", class: "btn btn-small", "aria-label": `Retry ${t.name}`, onclick: (ev) => onRetry(ev.currentTarget, t) }, "Retry") : null,
        stall && t.magnet ? h("button", { type: "button", class: "btn btn-small", "aria-label": `Re-add ${t.name}`, onclick: (ev) => onReadd(ev.currentTarget, t) }, "Re-add") : null,
        t.magnet ? h("button", { type: "button", class: "btn btn-small", "aria-label": `Copy magnet link of ${t.name}`, onclick: () => onCopyMagnet(t) }, "Copy magnet") : null,
        h("button", { type: "button", class: "btn btn-small", "aria-label": `Remove ${t.name}`, onclick: (ev) => onRemove(ev.currentTarget, t) }, "Remove"),
      ),
    ),
  );
}
