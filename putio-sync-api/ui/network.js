// Network settings (feature 037): what putio-sync listens on now, entry by entry, under
// the group's two lists. Before a save the lists are checked on the server; when the new
// API list would cut this page off, or nobody can tell, the save asks first and says
// where the UI will be.

import { get, post } from "./api.js";
import { confirm, fill, h } from "./app.js";

const reason = (e) => (e && (e.error || e.message)) || String(e);

const entryText = (e) => `${e.entry} → ${e.addresses ? e.addresses.join(", ") : `waiting: ${e.waiting}`}`;

/** The group's `extra`: the "Listening now" panel; it sends nothing of its own */
export function networkExtra() {
  const box = h("div", { class: "tailscale" }, h("p", { class: "muted", text: "Loading…" }));
  const load = async () => {
    let l;
    try {
      l = await get("/listen");
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      box.replaceChildren(h("p", { class: "error-text", text: `Listen addresses unavailable: ${reason(e)}` }), h("button", { type: "button", class: "btn btn-small", onclick: load }, "Retry"));
      return;
    }
    const list = (entries) => h("ul", { class: "tailscale-urls" }, entries.map((e) => h("li", { text: entryText(e) })));
    const notes = [];
    if (l.server_flags) notes.push("This server was started with --bind or --port, which take the place of the API list.");
    if (l.api.env_override) notes.push("PUTIO_SYNC_API_BIND is set and takes the place of the API list.");
    if (l.api.tailnet_only) notes.push("Tailnet only is on (Tailscale settings): the API listens on the tailnet address at these ports.");
    if (l.metrics.env_override) notes.push("PUTIO_SYNC_METRICS_BIND is set and takes the place of the metrics list.");
    fill(
      box,
      h("h3", { text: "Listening now" }),
      h("p", { class: "muted small", text: l.api.listening.length ? `API: ${l.api.listening.join(", ")}` : "API: no address held by this process" }),
      list(l.api.entries),
      l.metrics.enabled ? h("p", { class: "muted small", text: "Metrics:" }) : h("p", { class: "muted small", text: "Metrics are off." }),
      l.metrics.enabled ? list(l.metrics.entries) : null,
      ...notes.map((n) => h("p", { class: "muted small", text: n })),
      h("p", { class: "muted small", text: "Entries: ip:port, [ipv6]:port, interface:port (e.g. eth0:15432) or tailnet:port. Changes apply after a restart." }),
    );
  };
  load();
  return { nodes: box, read: () => ({}) };
}

const same = (a, b) => a.length === b.length && a.every((x, i) => x === b[i]);

/** Before a save: check the lists, and ask when the API list may cut this page off */
export async function networkBeforeSave(body, data) {
  let c;
  try {
    c = await post("/settings/network/check", body);
  } catch (e) {
    return true; // the save reports its own error
  }
  if (!c.valid) return true;
  const warn = (c.warnings || []).join(" ");
  if (same(body.api_bind || [], data.api_bind || []) && !warn) return true;
  if (c.this_page === "kept" && !warn) return true;
  const at = c.ui_at && c.ui_at.length ? ` After a restart the UI is at ${c.ui_at.join(", ")}.` : "";
  const text =
    c.this_page === "cut_off"
      ? `This page came in on ${c.listened_on}, which the new list doesn't listen on: after a restart it won't reach putio-sync.${at}`
      : c.this_page === "unknown"
        ? `putio-sync can't tell whether this page will still reach it after a restart.${at}`
        : at.trim();
  return confirm({ title: "Save these listen addresses?", text: [text, warn].filter(Boolean).join(" "), verb: "Save" });
}
