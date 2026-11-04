// Tailscale settings (feature 036): what Tailscale says about this machine (state, MagicDNS
// name, the addresses that open putio-sync over the tailnet, hints), under the group's
// fields. "Allow me" fills the allowed users with this machine's Tailscale login; turning
// "tailnet only" on asks first, since the LAN loses the API after a restart.

import { get } from "./api.js";
import { confirm, h } from "./app.js";

const STATES = {
  running: ["ok", "Running"],
  starting: ["warn", "Starting"],
  stopped: ["warn", "Stopped"],
  needs_login: ["warn", "Signed out"],
  unreachable: ["bad", "Not answering"],
  not_found: ["", "Not found"],
};

const reason = (e) => (e && (e.error || e.message)) || String(e);

/** The group's `extra`: the details panel; it sends nothing of its own */
export function tailscaleExtra() {
  const box = h("div", { class: "tailscale" }, h("p", { class: "muted", text: "Asking Tailscale…" }));
  const load = async () => {
    let t;
    try {
      t = await get("/tailscale");
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      box.replaceChildren(h("p", { class: "error-text", text: `Tailscale unavailable: ${reason(e)}` }), h("button", { type: "button", class: "btn btn-small", onclick: load }, "Retry"));
      return;
    }
    render(t);
  };

  function render(t) {
    const [tone, label] = STATES[t.state] || ["", t.state];
    const kv = h("dl", { class: "kv" }, h("dt", { text: "Tailscale" }), h("dd", {}, h("span", { class: `badge ${tone}`, text: label }), t.version ? ` ${t.version}` : null));
    const row = (k, v) => v && kv.append(h("dt", { text: k }), h("dd", { text: v }));
    row("Name", t.dns_name || t.host_name);
    row("Tailnet", t.tailnet);
    row("Address", (t.addresses || []).join(", "));
    row("Tailnet only would listen on", t.tailnet_bind);
    const urls = t.urls && t.urls.length ? h("ul", { class: "tailscale-urls" }, t.urls.map((u) => h("li", {}, h("a", { href: u, target: "_blank", rel: "noopener", text: u })))) : null;
    const hints = (t.hints || []).map((x) => h("p", { class: "muted small", text: x }));
    const allow = t.owner_login
      ? h("button", { type: "button", class: "btn btn-small", title: "Add this machine's Tailscale login to the allowed users", onclick: (ev) => allowOwner(ev.currentTarget, t.owner_login) }, `Allow ${t.owner_login}`)
      : null;
    box.replaceChildren(h("h3", { text: "This machine" }), kv, urls ? h("p", { class: "muted small", text: "Open putio-sync over the tailnet:" }) : null, urls, ...hints, allow ? h("div", { class: "toolbar spaced" }, allow) : null);
  }

  load();
  return { nodes: box, read: () => ({}) };
}

/** Add `login` to the allowed users field unless it is there; the form previews it */
function allowOwner(button, login) {
  const area = button.form && button.form.querySelector("#set-tailscale-allowed_users");
  if (!area) return;
  const users = area.value.split("\n").map((s) => s.trim()).filter(Boolean);
  if (!users.some((u) => u.toLowerCase() === login.toLowerCase())) users.push(login);
  area.value = users.join("\n");
  area.dispatchEvent(new Event("input", { bubbles: true }));
}

/** Before a save: ask when "tailnet only" is being turned on */
export function tailscaleBeforeSave(body, data) {
  if (!body.tailnet_only || data.tailnet_only) return Promise.resolve(true);
  return confirm({
    title: "Listen on the tailnet only?",
    text: "After a restart, putio-sync answers only on its Tailscale address. Devices on the LAN that aren't on the tailnet lose the API and this page. If Tailscale is down at start, putio-sync waits for it instead of listening elsewhere.",
    verb: "Listen on the tailnet only",
  });
}
