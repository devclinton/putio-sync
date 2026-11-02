// Local files page (feature 044): browse the download folder and each watch folder on
// the NAS, with partial downloads and rejected watch files marked. Delete (confirmed,
// with a folder's size), rename in place, and "Re-check" a rejected watch file so the
// watcher looks at it again. The server keeps every path inside its root.
// Route: #/files/<root>?path=<folder>

import { del, get, post } from "../api.js";
import { confirm, empty, h, navigate, poll, toast } from "../app.js";
import { bytes, dateTime } from "../format.js";

const REFRESH_MS = 10000;

let els = null;
let roots = [];
let root = null;
let path = "";
let entries = [];
let editing = null; // the path being renamed; refreshes wait until it is done
let refresh = null;
let seq = 0;

export function mount(el, { path: route = [], query } = {}) {
  root = route[0] || null;
  path = (query && query.get("path")) || "";
  entries = [];
  editing = null;
  els = {
    roots: h("div", { class: "toolbar", role: "group", "aria-label": "Folder" }),
    crumbs: h("nav", { class: "crumbs", "aria-label": "Path" }),
    list: h("section", { class: "card wide" }, h("p", { class: "muted", text: "Loading…" })),
  };
  el.append(
    h("div", { class: "page-head" }, h("div", {}, h("h1", { text: "Local files" }), els.crumbs), els.roots),
    h("div", { class: "stack" }, els.list),
  );
  refresh = poll(load, REFRESH_MS);
}

export function unmount() {
  els = null;
  refresh = null;
  seq += 1;
}

const href = (key, p) => `#/files/${encodeURIComponent(key)}${p ? `?${new URLSearchParams({ path: p })}` : ""}`;

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
  const title = `${n} ${n === 1 ? "item" : "items"}${truncated ? " (the first only; this folder has more)" : ""}`;
  if (n === 0) {
    els.list.replaceChildren(h("h2", { text: "Empty" }), empty("Nothing in this folder"));
    return;
  }
  els.list.replaceChildren(
    h("h2", { text: title }),
    h(
      "table",
      { class: "table" },
      h("thead", {}, h("tr", {}, ["Name", "Kind", "Size", "Modified"].map((t) => h("th", { class: t === "Size" ? "right" : null, text: t })), h("th", { class: "right", text: "Actions" }))),
      h("tbody", {}, entries.map((e) => row(current, e))),
    ),
  );
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
    actions.push(
      h("button", { type: "button", class: "btn btn-small", "aria-label": `Rename ${e.name}`, onclick: () => onRename(e) }, "Rename"),
      h("button", { type: "button", class: "btn btn-small", "aria-label": `Delete ${e.name}`, onclick: (ev) => onDelete(ev.currentTarget, e) }, "Delete"),
    );
  }
  return h(
    "tr",
    { dataset: { path: e.path } },
    h("td", { class: "name-cell name", "data-label": "Name" }, name, e.partial ? h("small", { class: "muted", text: ` → ${e.partial}` }) : null),
    h("td", { "data-label": "Kind" }, badges(e)),
    h("td", { class: "num right nowrap", "data-label": "Size", text: e.kind === "dir" ? "—" : bytes(e.size) }),
    h("td", { class: "nowrap", "data-label": "Modified", text: dateTime(e.modified) }),
    h("td", { class: "actions-cell", "data-label": "Actions" }, h("div", { class: "row-actions" }, actions)),
  );
}
