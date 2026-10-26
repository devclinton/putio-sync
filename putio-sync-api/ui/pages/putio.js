// Put.io page: browse folders from the root, filter the current folder, queue a file or
// folder for download, delete from Put.io (confirmed; hidden for files the queue still
// needs, and the server refuses those too).

import { del, get, post } from "../api.js";
import { confirm, h, toast } from "../app.js";
import { bytes, dateTime, utc } from "../format.js";

const ROOT = { id: 0, name: "Put.io" };
const KEEP = new Set(["pending", "downloading", "paused"]);

// The breadcrumb survives route changes, so descending and going back keep the names
let crumbs = [ROOT];
let folder = 0;
let files = [];
let queued = new Set();
let seq = 0;
let els = null;

export function mount(el, { path = [] } = {}) {
  folder = Number.parseInt(path[0], 10) || 0;
  const at = crumbs.findIndex((c) => c.id === folder);
  if (folder === 0) crumbs = [ROOT];
  else if (at >= 0) crumbs = crumbs.slice(0, at + 1);
  else crumbs = [ROOT, { id: null, name: "…" }, { id: folder, name: String(folder) }]; // a direct link

  files = [];
  els = {
    crumbs: h("nav", { class: "crumbs", "aria-label": "Folder" }),
    filter: h("input", { type: "search", class: "input", placeholder: "Filter by name", "aria-label": "Filter by name", oninput: renderList }),
    details: h("section", { class: "card", hidden: true }),
    list: h("section", { class: "card wide" }, h("p", { class: "muted", text: "Loading…" })),
  };
  renderCrumbs();
  el.append(
    h("div", { class: "page-head" }, h("div", {}, h("h1", { text: "Put.io" }), els.crumbs), h("div", { class: "toolbar" }, els.filter)),
    h("div", { class: "stack" }, els.details, els.list),
  );
  load();
}

export function unmount() {
  els = null;
  seq += 1;
}

// ---- Data -------------------------------------------------------------------

async function load() {
  const mine = ++seq;
  const [listing] = await Promise.all([get(`/putio/files?parent_id=${folder}`).catch((err) => ({ err })), loadQueue()]);
  if (mine !== seq || !els) return;
  if (listing.err) {
    if (listing.err.status === 401 || listing.err.status === 429) return;
    els.list.replaceChildren(
      h("h2", { text: "Can't list this folder" }),
      h("p", { class: "error-text", text: reason(listing.err) }),
      h("button", { type: "button", class: "btn spaced", onclick: retry }, "Retry"),
    );
    return;
  }
  files = listing.items || [];
  renderList();
}

function retry() {
  els.list.replaceChildren(h("p", { class: "muted", text: "Loading…" }));
  load();
}

/** File ids the queue still needs; a failed fetch leaves the guard to the server. */
async function loadQueue() {
  try {
    const q = await get("/local/queue");
    queued = new Set((q.items || []).filter((i) => KEEP.has(String(i.status).toLowerCase())).map((i) => i.file_id));
  } catch {
    queued = new Set();
  }
}

function reason(e) {
  return e.details ? `${e.error}: ${e.details}` : e.error;
}

// ---- Actions ----------------------------------------------------------------

async function onDownload(button, f) {
  button.disabled = true;
  try {
    const r = await post("/local/queue", { file_id: f.id });
    const parts = [`found ${r.found}`, `queued ${r.queued}`, `skipped ${r.skipped}`, `blocked ${r.blocked}`];
    if (r.failed) parts.push(`failed ${r.failed}`);
    toast(parts.join(", "), r.failed ? "error" : "ok");
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toast(`Couldn't queue ${f.name}`, "error", reason(e));
  }
  if (button.isConnected) button.disabled = false;
  await loadQueue();
  if (els) renderList();
}

async function onDelete(button, f) {
  const ok = await confirm({ title: "Delete from Put.io", text: `Delete ${f.name} from Put.io? This cannot be undone.`, verb: "Delete" });
  if (!ok) return;
  button.disabled = true;
  try {
    const r = await del(`/putio/files/${f.id}`);
    toast((r && r.message) || `Deleted ${f.name}`);
    load();
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toast(`Couldn't delete ${f.name}`, "error", reason(e));
    if (button.isConnected) button.disabled = false;
  }
}

async function onDetails(f) {
  const box = els.details;
  box.hidden = false;
  box.replaceChildren(h("h2", { text: "Details" }), h("p", { class: "muted", text: "Loading…" }));
  let d;
  try {
    d = await get(`/putio/files/${f.id}`);
  } catch (e) {
    d = { err: e };
  }
  if (!els || box !== els.details) return;
  const close = h("button", { type: "button", class: "btn btn-small", onclick: () => (box.hidden = true) }, "Close");
  if (d.err) {
    box.replaceChildren(h("h2", { text: "Details" }), h("p", { class: "error-text", text: reason(d.err) }), close);
    return;
  }
  const row = (k, v) => [h("dt", { text: k }), h("dd", { class: "name", text: v })];
  box.replaceChildren(
    h("h2", { text: "Details" }),
    h("dl", { class: "kv" }, row("Name", d.name), row("ID", String(d.id)), row("Type", d.file_type), row("Content type", d.content_type || "—"), row("Size", bytes(d.size)), row("Created", dateTime(utc(d.created_at))), row("Queue", queued.has(d.id) ? "queued" : "—")),
    h("div", { class: "toolbar spaced" }, close),
  );
}

// ---- Rendering --------------------------------------------------------------

const isFolder = (f) => f.file_type === "FOLDER";

function renderCrumbs() {
  const last = crumbs.length - 1;
  els.crumbs.replaceChildren(
    ...crumbs.flatMap((c, i) => [
      i ? h("span", { class: "muted", text: " / " }) : [],
      i === last || c.id == null ? h("span", { "aria-current": i === last ? "page" : null, text: c.name }) : h("a", { href: `#/putio/${c.id}`, text: c.name }),
    ]),
  );
}

function renderList() {
  if (!els) return;
  const q = els.filter.value.trim().toLowerCase();
  const shown = q ? files.filter((f) => f.name.toLowerCase().includes(q)) : files;
  const title = q ? `${shown.length} of ${files.length} items` : `${files.length} ${files.length === 1 ? "item" : "items"}`;
  if (shown.length === 0) {
    els.list.replaceChildren(h("h2", { text: title }), h("p", { class: "muted", text: files.length ? "Nothing matches the filter." : "This folder is empty." }));
    return;
  }
  els.list.replaceChildren(
    h("h2", { text: title }),
    h(
      "table",
      { class: "table" },
      h("thead", {}, h("tr", {}, ["Name", "Type", "Size", "Date"].map((t) => h("th", { class: t === "Size" ? "right" : null, text: t })), h("th", { class: "right", text: "Actions" }))),
      h("tbody", {}, shown.map(fileRow)),
    ),
  );
}

function fileRow(f) {
  const name = isFolder(f)
    ? h("a", { href: `#/putio/${f.id}`, onclick: () => crumbs.push({ id: f.id, name: f.name }), text: `${f.name}/` })
    : h("button", { type: "button", class: "link", onclick: () => onDetails(f), text: f.name });
  const keep = queued.has(f.id);
  return h(
    "tr",
    {},
    h("td", { class: "name-cell name", "data-label": "Name" }, name),
    h("td", { "data-label": "Type" }, h("span", { class: "badge", text: isFolder(f) ? "folder" : (f.file_type || "file").toLowerCase() })),
    h("td", { class: "num right", "data-label": "Size", text: bytes(f.size) }),
    h("td", { "data-label": "Date", text: dateTime(utc(f.created_at)) }),
    h(
      "td",
      { class: "actions-cell", "data-label": "Actions" },
      h(
        "div",
        { class: "row-actions" },
        h("button", { type: "button", class: "btn btn-small", "aria-label": `Download ${f.name}`, onclick: (ev) => onDownload(ev.currentTarget, f) }, "Download"),
        keep
          ? h("span", { class: "badge active", title: "In the queue; skip or cancel it to delete", text: "queued" })
          : h("button", { type: "button", class: "btn btn-small", "aria-label": `Delete ${f.name}`, onclick: (ev) => onDelete(ev.currentTarget, f) }, "Delete"),
      ),
    ),
  );
}
