// Local files page (feature 044): browse the download folder and each watch folder on
// the NAS, with partial downloads and rejected watch files marked. Delete (confirmed,
// with a folder's size), rename in place, and "Re-check" a rejected watch file so the
// watcher looks at it again. The server keeps every path inside its root. Feature 045:
// filter (globs with AND, OR and NOT), sort by a heading, and tick rows (or Select all of
// what the filter shows) to delete several at once. Feature 034: "Verify" checks the
// ticked files (or everything under ticked folders) against Put.io; History shows the run.
// Route: #/files/<root>?path=<folder>&q=<filter>&sort=<key>-<asc|desc>

import { del, get, post, startVerify } from "../api.js";
import { confirm, empty, h, navigate, poll, toast } from "../app.js";
import { fetchPlan, fetchState, translatePlan } from "../subtitles.js";
import { bytes, dateTime } from "../format.js";
import { listControls } from "../listctl.js";
import { selection } from "../select.js";

const REFRESH_MS = 10000;

let els = null;
let roots = [];
let root = null;
let path = "";
let entries = [];
let editing = null; // the path being renamed; refreshes wait until it is done
let refresh = null;
let seq = 0;
let ctl = null;
let sel = null;
let shown = null; // the root and truncated flag of the last listing, to redraw it

const COLUMNS = {
  name: { label: "Name", value: (e) => e.name },
  size: { label: "Size", value: (e) => (e.kind === "dir" ? null : e.size) },
  modified: { label: "Modified", value: (e) => (e.modified ? Date.parse(e.modified) : null) },
};

const isDir = (e) => e.kind === "dir";

/** Only entries with their own Delete button can be ticked */
const canAct = (e) => e.valid_name && !e.in_use;

export function mount(el, { path: route = [], query } = {}) {
  root = route[0] || null;
  path = (query && query.get("path")) || "";
  entries = [];
  editing = null;
  shown = null;
  ctl = listControls({ query, columns: COLUMNS, onChange: () => shown && !editing && renderList(...shown) });
  sel = selection({
    actions: [
      h("button", { type: "button", class: "btn btn-small", onclick: onVerifySelected }, "Verify"),
      h("button", { type: "button", class: "btn btn-small", onclick: onDeleteSelected }, "Delete"),
    ],
  });
  els = {
    roots: h("div", { class: "toolbar", role: "group", "aria-label": "Folder" }),
    crumbs: h("nav", { class: "crumbs", "aria-label": "Path" }),
    list: h("section", { class: "card wide" }, h("p", { class: "muted", text: "Loading…" })),
  };
  el.append(
    h("div", { class: "page-head" }, h("div", {}, h("h1", { text: "Local files" }), els.crumbs), els.roots),
    h("div", { class: "stack" }, h("div", { class: "toolbar" }, ctl.filter, ctl.menu), els.list),
  );
  refresh = poll(load, REFRESH_MS);
}

export function unmount() {
  els = null;
  refresh = null;
  ctl = null;
  sel = null;
  seq += 1;
}

/** A folder's address; it keeps the sort and drops the filter */
function href(key, p) {
  const params = new URLSearchParams(p ? { path: p } : {});
  const sort = ctl && new URLSearchParams(ctl.carry()).get("sort");
  if (sort) params.set("sort", sort);
  const qs = params.toString();
  return `#/files/${encodeURIComponent(key)}${qs ? `?${qs}` : ""}`;
}

const reason = (e) => (e.details ? `${e.error}: ${e.details}` : e.error);

const quiet = (e) => e.status === 401 || e.status === 429;

// ---- Data -------------------------------------------------------------------

async function load() {
  if (editing) return;
  const mine = ++seq;
  roots = (await get("/local/files")).roots || [];
  if (mine !== seq || !els) return;
  const current = roots.find((r) => r.key === root);
  if (!current) {
    // Unknown or no root picked: the first one
    if (roots.length) navigate(href(roots[0].key, ""));
    return;
  }
  renderRoots(current);
  if (!current.exists) {
    entries = [];
    shown = null;
    els.list.replaceChildren(empty(`${current.path} is not there`, "Check the folder in Settings, or that the share is mounted."));
    return;
  }
  let listing;
  try {
    listing = await get(`/local/files/${encodeURIComponent(root)}?${new URLSearchParams({ path })}`);
  } catch (e) {
    if (mine !== seq || !els || quiet(e)) return;
    if (e.network) throw e;
    els.list.replaceChildren(
      h("h2", { text: "Can't list this folder" }),
      h("p", { class: "error-text", text: reason(e) }),
      h("a", { class: "btn spaced", href: href(root, "") }, "Back to the top"),
    );
    return;
  }
  if (mine !== seq || !els || editing) return;
  entries = listing.entries || [];
  shown = [current, listing.truncated];
  renderList(current, listing.truncated);
}

const reload = () => refresh && refresh.now();

// ---- Actions ----------------------------------------------------------------

async function onDelete(button, e) {
  const what = e.kind === "dir" ? "folder" : e.kind === "link" ? "link" : "file";
  let detail = "";
  if (e.kind === "dir") {
    try {
      const s = await get(`/local/files/${encodeURIComponent(root)}/summary?${new URLSearchParams({ path: e.path })}`);
      detail = ` and everything in it (${s.files} ${s.files === 1 ? "file" : "files"}, ${bytes(s.bytes)})`;
    } catch (err) {
      if (!quiet(err)) toast(`Couldn't look inside ${e.name}`, "error", reason(err));
      return;
    }
  }
  const linkNote = e.kind === "link" ? " Only the link goes; what it points to stays." : "";
  const ok = await confirm({ title: `Delete ${what}`, text: `Delete ${e.path}${detail} from the NAS? This cannot be undone.${linkNote}`, verb: "Delete" });
  if (!ok) return;
  button.disabled = true;
  try {
    const r = await del(`/local/files/${encodeURIComponent(root)}?${new URLSearchParams({ path: e.path })}`);
    toast(r.message);
  } catch (err) {
    if (!quiet(err)) toast(`Couldn't delete ${e.name}`, "error", reason(err));
  }
  reload();
}

async function onRequeue(button, e) {
  button.disabled = true;
  try {
    const r = await post(`/local/files/${encodeURIComponent(root)}/requeue`, { path: e.path });
    toast(r.message);
  } catch (err) {
    if (!quiet(err)) toast(`Couldn't re-check ${e.name}`, "error", reason(err));
  }
  reload();
}

/** Ask what subtitles could be fetched, confirm when it matters, then start the item */
async function onFetchSubtitles(button, e) {
  button.disabled = true;
  const base = `/local/files/${encodeURIComponent(root)}/subtitles`;
  try {
    const plan = fetchPlan(e.name, await get(`${base}?path=${encodeURIComponent(e.path)}`));
    if (!plan.go) return toast(plan.message, "error");
    if (plan.confirm && !(await confirm(plan.confirm))) return;
    const r = await post(base, { path: e.path });
    toast(`Fetching subtitles for ${e.name}`, "ok", `Follow it on the Active page as ${r.key}`);
  } catch (err) {
    if (!quiet(err)) toast(`Couldn't fetch subtitles for ${e.name}`, "error", reason(err));
  } finally {
    if (button.isConnected) button.disabled = false;
  }
}

/** Ask what could be translated, confirm the languages, then start the item */
async function onTranslate(button, e) {
  button.disabled = true;
  const base = `/local/files/${encodeURIComponent(root)}/translate`;
  try {
    const plan = translatePlan(e.name, await get(`${base}?path=${encodeURIComponent(e.path)}`));
    if (!plan.go) return toast(plan.message, "error");
    if (!(await confirm(plan.confirm))) return;
    const r = await post(base, { path: e.path, targets: plan.targets });
    toast(`Translating ${e.name}`, "ok", `Follow it on the Active page as ${r.key}`);
  } catch (err) {
    if (!quiet(err)) toast(`Couldn't translate ${e.name}`, "error", reason(err));
  } finally {
    if (button.isConnected) button.disabled = false;
  }
}

function onRename(e) {
  editing = e.path;
  const input = h("input", { class: "input", value: e.name, "aria-label": `New name for ${e.name}` });
  const done = () => {
    editing = null;
    reload();
  };
  const save = async () => {
    const name = input.value.trim();
    if (!name || name === e.name) return done();
    try {
      const r = await post(`/local/files/${encodeURIComponent(root)}/rename`, { path: e.path, name });
      toast(r.message);
    } catch (err) {
      if (!quiet(err)) toast(`Couldn't rename ${e.name}`, "error", reason(err));
      input.focus();
      return;
    }
    done();
  };
  input.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter") save();
    else if (ev.key === "Escape") done();
  });
  const row = els.list.querySelector(`tr[data-path="${CSS.escape(e.path)}"]`);
  row.querySelector(".name-cell").replaceChildren(input);
  row.querySelector(".row-actions").replaceChildren(
    h("button", { type: "button", class: "btn btn-small btn-primary", onclick: save }, "Save"),
    h("button", { type: "button", class: "btn btn-small", onclick: done }, "Cancel"),
  );
  input.focus();
  input.setSelectionRange(0, e.name.lastIndexOf(".") > 0 ? e.name.lastIndexOf(".") : e.name.length);
}

const SHOWN_NAMES = 5;

/** Up to five names, then "and N more". */
function some(names) {
  const more = names.length > SHOWN_NAMES ? `, and ${names.length - SHOWN_NAMES} more` : "";
  return names.slice(0, SHOWN_NAMES).join(", ") + more;
}

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

/** One confirm with the count, then a delete per selected entry; one toast sums them. */
async function onDeleteSelected() {
  const picked = entries.filter((e) => sel.has(e.path));
  const n = picked.length;
  const folders = picked.some(isDir) ? " Folders go with everything in them." : "";
  const ok = await confirm({ title: "Delete from the NAS", text: `Delete ${plural(n, "item", "items")} (${some(picked.map((e) => e.name))}) from the NAS?${folders} This cannot be undone.`, verb: "Delete" });
  if (!ok || !sel) return;
  const page = sel;
  page.busy(true);
  const failed = [];
  let deleted = 0;
  for (const e of picked) {
    try {
      await del(`/local/files/${encodeURIComponent(root)}?${new URLSearchParams({ path: e.path })}`);
      deleted += 1;
    } catch (err) {
      if (quiet(err)) break;
      failed.push(`${e.name} (${reason(err)})`);
    }
  }
  toast(`Deleted ${plural(deleted, "item", "items")}`, failed.length ? "error" : "ok", failed.length ? `Couldn't delete ${some(failed)}` : null);
  page.busy(false);
  page.clear();
  reload();
}

/** Start a verify run over the ticked entries, by their full paths on the NAS */
async function onVerifySelected() {
  const current = roots.find((r) => r.key === root);
  if (!current || !sel) return;
  const base = String(current.path).replace(/[\\/]+$/, "");
  const paths = entries.filter((e) => sel.has(e.path)).map((e) => `${base}/${e.path}`);
  try {
    await startVerify(paths);
    toast(`Verifying ${plural(paths.length, "item", "items")}`, "ok", "History shows the run.");
    sel.clear();
  } catch (err) {
    if (!quiet(err)) toast("Couldn't start verifying", "error", reason(err));
  }
}

// ---- Rendering --------------------------------------------------------------

function renderRoots(current) {
  els.roots.replaceChildren(
    ...roots.map((r) =>
      h("a", { class: r.key === current.key ? "btn btn-primary" : "btn", href: href(r.key, ""), "aria-current": r.key === current.key ? "page" : null, title: r.path, text: r.label }),
    ),
  );
  const parts = path ? path.split("/") : [];
  els.crumbs.replaceChildren(
    parts.length ? h("a", { href: href(root, ""), text: current.path }) : h("span", { "aria-current": "page", text: current.path }),
    ...parts.flatMap((p, i) => [
      h("span", { class: "muted", text: " / " }),
      i === parts.length - 1 ? h("span", { "aria-current": "page", text: p }) : h("a", { href: href(root, parts.slice(0, i + 1).join("/")), text: p }),
    ]),
  );
}

function renderList(current, truncated) {
  const n = entries.length;
  const rows = ctl.apply(entries, { folder: isDir });
  const count = ctl.filtering ? `${rows.length} of ${n} items` : `${n} ${n === 1 ? "item" : "items"}`;
  const title = `${count}${truncated ? " (the first only; this folder has more)" : ""}`;
  sel.reset(rows.filter(canAct).map((e) => e.path));
  if (rows.length === 0) {
    els.list.replaceChildren(h("h2", { text: n ? title : "Empty" }), n ? empty("Nothing matches the filter", "Try fewer terms, or * around a word.") : empty("Nothing in this folder"));
    sel.update();
    return;
  }
  els.list.replaceChildren(
    h("h2", { text: title }),
    sel.bar,
    h(
      "table",
      { class: "table" },
      h("thead", {}, h("tr", {}, h("th", { class: "select-cell" }, h("span", { class: "visually-hidden", text: "Select" })), ctl.th("name"), h("th", { text: "Kind" }), ctl.th("size", { class: "right" }), ctl.th("modified"), h("th", { class: "right", text: "Actions" }))),
      h("tbody", {}, rows.map((e) => row(current, e))),
    ),
  );
  sel.update();
}

function badges(e) {
  const list = [h("span", { class: "badge", text: e.kind === "dir" ? "folder" : e.kind })];
  if (e.partial) list.push(h("span", { class: "badge warn", title: `Partial download of ${e.partial}`, text: "partial" }));
  if (e.rejected) list.push(h("span", { class: "badge bad", title: `Rejected by the release check; was ${e.rejected}`, text: "rejected" }));
  if (e.in_use) list.push(h("span", { class: "badge active", title: "A download is writing here; cancel it in the Queue to delete", text: "downloading" }));
  return h("span", { class: "badges" }, list);
}

function row(current, e) {
  const name = e.kind === "dir" && e.valid_name ? h("a", { href: href(root, e.path), text: `${e.name}/` }) : h("span", { text: e.name });
  const actions = [];
  if (e.valid_name && !e.in_use) {
    if (current.watch && e.rejected && /\.(torrent|magnet)$/.test(e.rejected)) {
      actions.push(h("button", { type: "button", class: "btn btn-small", title: `Rename back to ${e.rejected} so the watcher checks it again`, onclick: (ev) => onRequeue(ev.currentTarget, e) }, "Re-check"));
    }
    const fetch = fetchState(e);
    if (fetch.show) {
      actions.push(h("button", { type: "button", class: "btn btn-small", "aria-label": `Fetch subtitles for ${e.name}`, disabled: fetch.disabled, title: fetch.why || "Get subtitles for this video", onclick: (ev) => onFetchSubtitles(ev.currentTarget, e) }, "Fetch subtitles"));
      actions.push(h("button", { type: "button", class: "btn btn-small", "aria-label": `Translate subtitles of ${e.name}`, disabled: fetch.disabled, title: "Translate this video's subtitle into the wanted languages", onclick: (ev) => onTranslate(ev.currentTarget, e) }, "Translate"));
    }
    actions.push(
      h("button", { type: "button", class: "btn btn-small", "aria-label": `Rename ${e.name}`, onclick: () => onRename(e) }, "Rename"),
      h("button", { type: "button", class: "btn btn-small", "aria-label": `Delete ${e.name}`, onclick: (ev) => onDelete(ev.currentTarget, e) }, "Delete"),
    );
  }
  return h(
    "tr",
    { dataset: { path: e.path } },
    sel.box(e.path, { label: `Select ${e.name}`, disabled: !canAct(e), title: e.in_use ? "A download is writing here; cancel it in the Queue to delete" : null }),
    h("td", { class: "name-cell name", "data-label": "Name" }, name, e.partial ? h("small", { class: "muted", text: ` → ${e.partial}` }) : null),
    h("td", { "data-label": "Kind" }, badges(e)),
    h("td", { class: "num right nowrap", "data-label": "Size", text: e.kind === "dir" ? "—" : bytes(e.size) }),
    h("td", { class: "nowrap", "data-label": "Modified", text: dateTime(e.modified) }),
    h("td", { class: "actions-cell", "data-label": "Actions" }, h("div", { class: "row-actions" }, actions)),
  );
}
