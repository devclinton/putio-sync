// Put.io page: browse folders from the root, filter the current folder, queue a file or
// folder for download, delete from Put.io (confirmed; hidden for files the queue still
// needs, and the server refuses those too), and remove empty folders account-wide (a dry
// run first, then a confirm with the count). Feature 008: tick rows (or Select all) to
// download or delete several at once; queued files can't be ticked. Feature 009: the
// folder's rule card, and "blocked" / "skipped" badges on files its rule won't download.
// Feature 042: flag a file as fake, and undo it. Feature 045: the filter takes globs with
// AND, OR and NOT, and the Name, Size and Date headings sort (folders stay first).

import { del, get, post } from "../api.js";
import { confirm, empty, h, toast } from "../app.js";
import { flagControl, loadFlags } from "../flags.js";
import { bytes, dateTime, utc } from "../format.js";
import { listControls } from "../listctl.js";
import { ruleCard } from "../rules.js";
import { selection } from "../select.js";

const ROOT = { id: 0, name: "Put.io" };
const KEEP = new Set(["pending", "downloading", "paused"]);

// The breadcrumb survives route changes, so descending and going back keep the names
let crumbs = [ROOT];
let folder = 0;
let files = [];
let queued = new Set();
let flags = null;
let seq = 0;
let els = null;
let sel = null;
let rule = null;
let ctl = null;

const COLUMNS = {
  name: { label: "Name", value: (f) => f.name },
  size: { label: "Size", value: (f) => f.size },
  date: { label: "Date", value: (f) => (f.created_at ? Date.parse(utc(f.created_at)) : null) },
};

export function mount(el, { path = [], query } = {}) {
  folder = Number.parseInt(path[0], 10) || 0;
  const at = crumbs.findIndex((c) => c.id === folder);
  if (folder === 0) crumbs = [ROOT];
  else if (at >= 0) crumbs = crumbs.slice(0, at + 1);
  else crumbs = [ROOT, { id: null, name: "…" }, { id: folder, name: String(folder) }]; // a direct link, until walkUp names it

  files = [];
  ctl = listControls({ query, columns: COLUMNS, onChange: renderList });
  els = {
    crumbs: h("nav", { class: "crumbs", "aria-label": "Folder" }),
    clean: h("button", { type: "button", class: "btn", title: "Find and delete empty folders in the whole Put.io account", onclick: (ev) => onCleanEmpty(ev.currentTarget) }, "Remove empty folders"),
    details: h("section", { class: "card", hidden: true }),
    list: h("section", { class: "card wide" }, h("p", { class: "muted", text: "Loading…" })),
  };
  sel = selection({
    actions: [
      h("button", { type: "button", class: "btn btn-small", onclick: onDownloadSelected }, "Download"),
      h("button", { type: "button", class: "btn btn-small", onclick: onDeleteSelected }, "Delete"),
    ],
  });
  rule = ruleCard(folder, renderList);
  renderCrumbs();
  if (crumbs[1] && crumbs[1].id == null) walkUp(folder);
  el.append(
    h("div", { class: "page-head" }, h("div", {}, h("h1", { text: "Put.io" }), els.crumbs), h("div", { class: "toolbar" }, ctl.filter, ctl.menu, els.clean)),
    h("div", { class: "stack" }, rule.el, els.details, els.list),
  );
  load();
}

export function unmount() {
  els = null;
  sel = null;
  rule = null;
  ctl = null;
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
  flags = await loadFlags(fileTargets());
  if (mine !== seq || !els) return;
  renderList();
}

const fileTargets = () => files.filter((f) => !isFolder(f)).map((f) => ({ file_id: f.id }));

/** After a flag changes: reload the flags only */
async function reflag() {
  const mine = seq;
  const next = await loadFlags(fileTargets());
  if (mine !== seq || !els) return;
  flags = next;
  renderList();
}

const MAX_DEPTH = 32;

/** Build the breadcrumb of a directly opened folder from each folder's `parent_id`. */
async function walkUp(id) {
  const page = els; // a new mount (or unmount) replaces it
  const chain = [];
  const seen = new Set();
  try {
    while (id > 0 && !seen.has(id) && chain.length < MAX_DEPTH) {
      seen.add(id);
      const f = await get(`/putio/files/${id}`);
      if (els !== page) return;
      chain.unshift({ id: f.id, name: f.name });
      id = f.parent_id;
    }
  } catch {
    return; // keep the placeholder; the listing shows any error
  }
  if (id > 0 || els !== page) return; // too deep or a loop: keep the placeholder
  crumbs = [ROOT, ...chain];
  renderCrumbs();
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

const SHOWN_NAMES = 5;

/** Up to five names, then "and N more". */
function some(names) {
  const more = names.length > SHOWN_NAMES ? `, and ${names.length - SHOWN_NAMES} more` : "";
  return names.slice(0, SHOWN_NAMES).join(", ") + more;
}

const selectedFiles = () => files.filter((f) => sel.has(f.id));

/** Queue each selected item in turn and sum the counts in one toast. */
async function onDownloadSelected() {
  const picked = selectedFiles();
  const page = sel;
  page.busy(true);
  const sum = { found: 0, queued: 0, skipped: 0, blocked: 0, failed: 0 };
  const errors = [];
  for (const f of picked) {
    try {
      const r = await post("/local/queue", { file_id: f.id });
      for (const k of Object.keys(sum)) sum[k] += r[k] || 0;
    } catch (e) {
      if (e.status === 401 || e.status === 429) break;
      sum.failed += 1;
      errors.push(f.name);
    }
  }
  const parts = [`found ${sum.found}`, `queued ${sum.queued}`, `skipped ${sum.skipped}`, `blocked ${sum.blocked}`];
  if (sum.failed) parts.push(`failed ${sum.failed}`);
  toast(`${plural(picked.length, "item", "items")}: ${parts.join(", ")}`, sum.failed ? "error" : "ok", errors.length ? `Couldn't queue ${some(errors)}` : null);
  page.busy(false);
  if (sel !== page) return;
  sel.clear();
  await loadQueue();
  if (els) renderList();
}

/** One confirm with the count, then one request for every selected item. */
async function onDeleteSelected() {
  const picked = selectedFiles();
  const n = picked.length;
  const ok = await confirm({
    title: "Delete from Put.io",
    text: `Delete ${plural(n, "item", "items")} from Put.io (${some(picked.map((f) => f.name))})? This cannot be undone.`,
    verb: "Delete",
  });
  if (!ok || !sel) return;
  const page = sel;
  page.busy(true);
  try {
    const r = await post("/putio/files/delete", { file_ids: picked.map((f) => f.id) });
    toast((r && r.message) || `Deleted ${plural(n, "item", "items")}`);
    page.clear();
    load();
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toast(`Couldn't delete ${plural(n, "item", "items")}`, "error", reason(e));
    // A 409 means the queue changed: refresh it so those rows can't be ticked
    await loadQueue();
    if (els) renderList();
  } finally {
    page.busy(false);
  }
}

const CLEANUP = "/putio/cleanup/empty-folders";
const SHOWN_PATHS = 5;

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

/** Dry run, confirm with the count, then the real run; the listing reloads after. */
async function onCleanEmpty(button) {
  button.disabled = true;
  const label = button.textContent;
  button.textContent = "Checking…";
  try {
    const preview = await post(CLEANUP, { dry_run: true });
    const n = preview.removed_count;
    if (n === 0) {
      const failed = preview.failures.length;
      toast("No empty folders to remove", failed ? "error" : "ok", failed ? `${plural(failed, "folder", "folders")} couldn't be checked` : null);
      return;
    }
    const paths = preview.removed.slice(-SHOWN_PATHS).map((r) => r.path);
    const more = n > paths.length ? `, and ${n - paths.length} more` : "";
    const ok = await confirm({
      title: "Remove empty folders",
      text: `Delete ${plural(n, "empty folder", "empty folders")} from Put.io (${paths.join(", ")}${more})? This cannot be undone.`,
      verb: "Delete",
    });
    if (!ok) return;
    button.textContent = "Deleting…";
    const r = await post(CLEANUP, { dry_run: false });
    const failed = r.failures.length;
    toast(`Removed ${plural(r.removed_count, "empty folder", "empty folders")}`, failed ? "error" : "ok", failed ? `${plural(failed, "folder", "folders")} failed` : null);
    load();
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toast("Couldn't remove empty folders", "error", reason(e));
  } finally {
    if (button.isConnected) {
      button.disabled = false;
      button.textContent = label;
    }
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

/** A folder's address; it keeps the sort and drops the filter */
const folderHref = (id) => `#/putio/${id}${ctl && ctl.carry() ? `?${ctl.carry()}` : ""}`;

function renderCrumbs() {
  const last = crumbs.length - 1;
  els.crumbs.replaceChildren(
    ...crumbs.flatMap((c, i) => [
      i ? h("span", { class: "muted", text: " / " }) : [],
      i === last || c.id == null ? h("span", { "aria-current": i === last ? "page" : null, text: c.name }) : h("a", { href: folderHref(c.id), text: c.name }),
    ]),
  );
}

function renderList() {
  if (!els) return;
  const shown = ctl.apply(files, { folder: isFolder });
  const title = ctl.filtering ? `${shown.length} of ${files.length} items` : `${files.length} ${files.length === 1 ? "item" : "items"}`;
  // Only rows shown, and not needed by the queue, can stay selected
  sel.reset(shown.filter((f) => !queued.has(f.id)).map((f) => f.id));
  if (shown.length === 0) {
    els.list.replaceChildren(h("h2", { text: title }), files.length ? empty("Nothing matches the filter", "Try fewer terms, or * around a word.") : empty("This folder is empty"));
    sel.update();
    return;
  }
  els.list.replaceChildren(
    h("h2", { text: title }),
    sel.bar,
    h(
      "table",
      { class: "table" },
      h("thead", {}, h("tr", {}, h("th", { class: "select-cell" }, h("span", { class: "visually-hidden", text: "Select" })), ctl.th("name"), h("th", { text: "Type" }), ctl.th("size", { class: "right" }), ctl.th("date"), h("th", { class: "right", text: "Actions" }))),
      h("tbody", {}, shown.map(fileRow)),
    ),
  );
  sel.update();
}

function fileRow(f) {
  const name = isFolder(f)
    ? h("a", { href: folderHref(f.id), onclick: () => crumbs.push({ id: f.id, name: f.name }), text: `${f.name}/` })
    : h("button", { type: "button", class: "link", onclick: () => onDetails(f), text: f.name });
  const keep = queued.has(f.id);
  const verdict = !isFolder(f) && rule && rule.badge(f.name);
  return h(
    "tr",
    {},
    sel.box(f.id, { label: `Select ${f.name}`, disabled: keep, title: keep ? "In the queue; skip or cancel it to delete" : null }),
    h("td", { class: "name-cell name", "data-label": "Name" }, name),
    h(
      "td",
      { "data-label": "Type" },
      h("span", { class: "badges" }, h("span", { class: "badge", text: isFolder(f) ? "folder" : (f.file_type || "file").toLowerCase() }), verdict ? h("span", { class: "badge warn", title: verdict.why, text: verdict.badge }) : null),
    ),
    h("td", { class: "num right", "data-label": "Size", text: bytes(f.size) }),
    h("td", { "data-label": "Date", text: dateTime(utc(f.created_at)) }),
    h(
      "td",
      { class: "actions-cell", "data-label": "Actions" },
      h(
        "div",
        { class: "row-actions" },
        isFolder(f) ? null : flagControl(flags, { target: { file_id: f.id }, name: f.name, onChange: reflag }),
        h("button", { type: "button", class: "btn btn-small", "aria-label": `Download ${f.name}`, onclick: (ev) => onDownload(ev.currentTarget, f) }, "Download"),
        keep
          ? h("span", { class: "badge active", title: "In the queue; skip or cancel it to delete", text: "queued" })
          : h("button", { type: "button", class: "btn btn-small", "aria-label": `Delete ${f.name}`, onclick: (ev) => onDelete(ev.currentTarget, f) }, "Delete"),
      ),
    ),
  );
}
