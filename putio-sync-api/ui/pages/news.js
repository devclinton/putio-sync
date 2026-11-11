// What's new (feature 017): the changelog built into the daemon. checkNews() shows the
// entries an upgrade added in a panel once per page load, and closing it marks them seen;
// the page lists every release. Entry text is markdown with `code`, [links](url) and
// **bold** only, rendered as elements; everything else stays text. Above the notes, the
// update checker (feature 055) shows a newer release with its notes, from update.js.

import { get, post } from "../api.js";
import { empty, h, toast } from "../app.js";
import { linkParts } from "../settings/sections.js";

const linked = (text) => linkParts(text).map((p) => (typeof p === "string" ? p : h("a", { href: p.href, text: p.text })));

const PANEL_MAX = 20;
const REPO = "https://github.com/devclinton/putio-sync/blob/main/";
const INLINE = /`([^`]+)`|\[([^\]]+)\]\(([^)\s]+)\)|\*\*([^*]+)\*\*/g;

/** Entry markdown → nodes */
function inline(text) {
  const out = [];
  let at = 0;
  for (const m of text.matchAll(INLINE)) {
    out.push(...linked(text.slice(at, m.index)));
    at = m.index + m[0].length;
    if (m[1]) out.push(h("code", { text: m[1] }));
    else if (m[4]) out.push(h("strong", { text: m[4] }));
    // A page of this app (#/settings/…) opens here
    else if (m[3].startsWith("#/")) out.push(h("a", { href: m[3], text: m[2] }));
    else if (/^https?:\/\//.test(m[3])) out.push(h("a", { href: m[3], target: "_blank", rel: "noopener", text: m[2] }));
    else if (/^[\w.-][\w./-]*$/.test(m[3])) out.push(h("a", { href: REPO + m[3], target: "_blank", rel: "noopener", text: m[2] }));
    else out.push(m[0]);
  }
  out.push(...linked(text.slice(at)));
  return out;
}

/** Releases as headings, kinds and lists; `keep` picks entries, `limit` caps them */
export function notes(releases, keep = () => true, limit = Infinity) {
  let left = limit;
  const out = [];
  for (const r of releases) {
    const kinds = [];
    for (const k of r.kinds) {
      const entries = k.entries.filter(keep).slice(0, Math.max(left, 0));
      left -= entries.length;
      if (entries.length) {
        kinds.push(
          h("h3", { class: "sub", text: k.name }),
          h("ul", { class: "news" }, entries.map((e) => h("li", { class: e.new ? "new" : null }, inline(e.text)))),
        );
      }
    }
    if (kinds.length) {
      const title = r.name === "Unreleased" ? "In this build" : r.name;
      out.push(h("section", { class: "card" }, h("h2", { text: r.date ? `${title} · ${r.date}` : title }), kinds));
    }
  }
  return out;
}

const newIds = (data) => [...new Set(data.releases.flatMap((r) => r.kinds.flatMap((k) => k.entries.filter((e) => e.new).map((e) => e.id))))];

/**
 * Once per page load after sign-in: the panel, when an upgrade added entries. Resolves
 * false when the notes couldn't be fetched, so the caller can try again.
 */
export async function checkNews() {
  let data;
  try {
    data = await get("/whats-new");
  } catch {
    return false; // the notes are optional
  }
  if (!data.new_count) return true;
  const ids = newIds(data);
  const more = ids.length - PANEL_MAX;
  const dialog = h(
    "dialog",
    { class: "news-panel", "aria-labelledby": "news-title" },
    h(
      "form",
      { method: "dialog" },
      h("h2", { id: "news-title", text: "What's new" }),
      h("div", { class: "stack" }, notes(data.releases, (e) => e.new, PANEL_MAX)),
      h(
        "div",
        { class: "dialog-actions" },
        h("a", { class: "btn", href: "#/news", onclick: () => dialog.close() }, more > 0 ? `${more} more…` : "All notes"),
        h("button", { class: "btn btn-primary", value: "ok" }, "Got it"),
      ),
    ),
  );
  dialog.addEventListener("close", () => {
    dialog.remove();
    post("/whats-new/seen", { ids }).catch(() => {});
  });
  document.body.append(dialog);
  dialog.showModal();
  return true;
}

/** An undismissed newer release from the update checker, or null */
export const pending = (u) => (u && u.available && !u.available.dismissed ? u.available : null);

/**
 * Once per page load after sign-in: say so when the update checker knows a newer release
 * that hasn't been dismissed.
 */
export async function checkUpdate() {
  try {
    const a = pending(await get("/update"));
    if (a) toast(`putio-sync ${a.version} is available`, "ok", "See What's new, from the version on the Status page.");
  } catch {
    // no checker (setup mode) or offline: nothing to say
  }
}

export async function mount(el) {
  const summary = h("p", { class: "muted", text: "Loading…" });
  const update = h("div");
  const body = h("div", { class: "stack" });
  el.append(h("div", { class: "page-head" }, h("div", {}, h("h1", { text: "What's new" }), summary)), update, body);
  const loadUpdate = async () => {
    try {
      const u = await get("/update");
      const { updateCard } = await import("./update.js");
      if (update.isConnected) update.replaceChildren(updateCard(u, loadUpdate));
    } catch {
      update.replaceChildren(); // no checker (setup mode)
    }
  };
  loadUpdate();
  try {
    const data = await get("/whats-new");
    const commit = data.commit ? ` (${data.commit.slice(0, 12)})` : "";
    summary.textContent = `Running ${data.version}${commit}.`;
    const all = notes(data.releases);
    body.append(...(all.length ? all : [empty("No release notes", "This build has no changelog entries.")]));
  } catch (e) {
    summary.textContent = `Unavailable: ${e.message}`;
  }
}

export function unmount() {}
