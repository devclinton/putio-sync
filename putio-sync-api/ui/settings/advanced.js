// Network and access, and Advanced sections (feature 046): listen addresses, Tailscale sign-in,
// logging, and Backup and restore (feature 015).

import { exportSettings, importSettings } from "../api.js";
import { networkBeforeSave, networkExtra } from "../network.js";
import { tailscaleBeforeSave, tailscaleExtra } from "../tailscale.js";
import { confirm, h, toast } from "../app.js";
import { groupCard, previewNodes, quiet, reason } from "./form.js";

const LEVELS = ["trace", "debug", "info", "warn", "error"];
const TAILSCALE = {
  id: "tailscale",
  key: "tailscale",
  title: "Tailscale",
  fields: [
    ["identity_sign_in", "Tailscale sign-in", "Let the allowed Tailscale users in without the API token", "b"],
    ["allowed_users", "Allowed users", "Tailscale logins, one per line, e.g. alice@example.com", "l"],
    ["tailnet_only", "Tailnet only", "Listen on the Tailscale address only, so the LAN can't reach the API (daemon, after a restart)", "b"],
    ["socket", "tailscaled socket", "Empty for the default; in Docker, mount the host's socket", "s?"],
  ],
  advanced: ["socket"],
  extra: tailscaleExtra,
  beforeSave: tailscaleBeforeSave,
};
const NETWORK = {
  id: "network",
  key: "network",
  title: "Network",
  fields: [
    ["api_bind", "API addresses", "One per line: ip:port, [ipv6]:port, interface:port or tailnet:port (after a restart)", "l"],
    ["metrics_bind", "Metrics addresses", "One per line, same forms; must not share an address with the API (after a restart)", "l"],
  ],
  advanced: ["metrics_bind"],
  extra: networkExtra,
  beforeSave: networkBeforeSave,
};
const LOGGING = {
  id: "logging",
  key: "logging",
  title: "Logging",
  fields: [
    ["level", "Level", "How much is logged", LEVELS],
    ["console_format", "Console format", "", ["Compact", "Pretty", "Json"]],
    ["file_path", "Log file", "", "s"],
    ["module_filters", "Module filters", "One per line, e.g. putio_sync_download=debug", "l"],
    ["buffer_lines", "Lines kept for the Logs page", "100 to 50,000", "n", 100, 50000],
    ["console_enabled", "Console", "Log to the console", "b"],
    ["file_enabled", "File", "Log to the file", "b"],
  ],
  advanced: ["console_format", "module_filters", "buffer_lines"],
};

export const networkCards = (ctx) => [groupCard(NETWORK, ctx), groupCard(TAILSCALE, ctx)];
export const advancedCards = (ctx) => [groupCard(LOGGING, ctx), backupCard(ctx.remount)];

function backupCard(remount) {
  const text = h("textarea", { class: "input wide", rows: 5, spellcheck: "false", placeholder: "Paste a settings file, or choose one", "aria-label": "Settings file" });
  const out = h("div", { class: "preview", "aria-live": "polite", hidden: true });
  const apply = h("button", { type: "button", class: "btn btn-primary", disabled: true }, "Apply");
  let last = null;
  const show = (...nodes) => {
    out.replaceChildren(h("div", {}, nodes));
    out.hidden = false;
  };
  const check = async () => {
    apply.disabled = true;
    last = null;
    if (!text.value.trim()) return void (out.hidden = true);
    try {
      const p = await importSettings(text.value, false);
      last = p;
      const lists = [
        [p.ignored, "Left out (credentials stay as they are)"],
        [(p.unknown || []).map((u) => u.key), "Not known, ignored"],
      ].map(([keys, label]) => (keys && keys.length ? h("p", { class: "muted", text: `${label}: ${keys.join(", ")}` }) : null));
      show(h("h3", { text: "If you import" }), previewNodes(p), lists);
      apply.disabled = !p.valid || !p.changes.length;
    } catch (e) {
      if (!quiet(e)) show(h("p", { class: "error-text", text: reason(e) }));
    }
  };
  const file = h("input", {
    type: "file",
    accept: ".toml,text/plain",
    "aria-label": "Choose a settings file",
    onchange: async () => {
      const f = file.files[0];
      if (f) text.value = await f.text();
      check();
    },
  });
  let timer = 0;
  text.addEventListener("input", () => {
    clearTimeout(timer);
    timer = setTimeout(check, 400);
  });
  apply.addEventListener("click", async () => {
    const n = last ? last.changes.length : 0;
    if (!(await confirm({ title: "Import these settings?", text: `${n} setting${n === 1 ? "" : "s"} will change and be saved to the config file.`, verb: "Import" }))) return;
    apply.disabled = true;
    try {
      const p = await importSettings(text.value, true);
      toast(p.message || "Imported");
      remount();
    } catch (e) {
      if (!quiet(e)) show(h("p", { class: "error-text", text: reason(e) }));
    }
  });
  const download = async () => {
    try {
      const url = URL.createObjectURL(new Blob([await exportSettings()], { type: "text/plain" }));
      h("a", { href: url, download: "putio-sync-settings.toml" }).click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (e) {
      if (!quiet(e)) toast(reason(e), "error");
    }
  };
  return h(
    "section",
    { class: "card", id: "card-backup" },
    h("h2", { text: "Backup and restore" }),
    h("p", { class: "muted", text: "The file holds every setting except credentials and the Radarr / Sonarr section. Importing changes only the settings the file has; you see what changes first." }),
    h("div", { class: "form-foot" }, h("button", { type: "button", class: "btn", onclick: download }, "Export"), file),
    h("div", { class: "spaced" }, text),
    out,
    h("div", { class: "form-foot" }, apply),
  );
}
