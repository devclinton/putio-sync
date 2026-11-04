// Settings page: appearance (this browser only, via theme.js), then the download, watcher, Put.io cleanup, Put.io storage (033), Tailscale (036), logging and notification groups
// as forms built from each GET body. Save PUTs the whole group and shows the API's
// message verbatim; a 400 shows its error under the form. Tokens are shown only as set /
// not set. The Radarr / Sonarr card has the download client form, auto-setup, and the
// Applications list with Add, Edit, Delete and Test (feature 018).
// Feature 009: Folder rules, Presets and Recently blocked cards (rules.js).
// Feature 042: the Release check card, with per-category and per-watch-folder overrides
// and a read-only table of what the check does for each.
// Feature 015: each group previews a save as you type, says what each field affects and
// whether it applies now, has a Defaults button; a warnings banner and a Backup and
// restore card (export and import without credentials).

import {
  addArrApplication,
  arrStatus,
  deleteArrApplication,
  discoverArr,
  exportSettings,
  get,
  importSettings,
  previewArrSetup,
  previewSettings,
  put,
  runArrSetup,
  saveArrApplication,
  saveArrClient,
  settingsAssistant,
  testArrApplication,
  testArrForm,
} from "../api.js";
import { networkBeforeSave, networkExtra } from "../network.js";
import { tailscaleBeforeSave, tailscaleExtra } from "../tailscale.js";
import { confirm, h, toast } from "../app.js";
import { relative } from "../format.js";
import { settingsCards } from "../rules.js";

// Field kinds: b checkbox, n number, n? number or empty, s text, s? text or empty,
// l one entry per line, or an array of <select> values.
const LEVELS = ["trace", "debug", "info", "warn", "error"];
const GROUPS = [
  {
    key: "download",
    title: "Download",
    fields: [
      ["workers", "Workers", "Downloads that run at once", "n", 1],
      ["target_directory", "Download folder", "Where files are saved", "s"],
      ["buffer_size_mb", "Buffer (MB)", "Write buffer per download", "n", 1],
      ["chunk_size_mb", "Chunk size (MB)", "Empty downloads each file in one request", "n?", 1],
      ["bandwidth_limit_bytes_per_sec", "Bandwidth limit (bytes/s)", "Empty for unlimited", "n?", 1],
      ["max_retries", "Retries", "Attempts after a failed download", "n", 0],
      ["retry_initial_delay_secs", "First retry delay (s)", "Doubles on each retry", "n", 0],
      ["sort_order", "Sort order", "Order new files are queued in", ["ByDate", "ByName", "ByExtension", "BySize"]],
      ["progress_style", "Progress style", "Console progress bars", ["PerFile", "Overall", "Individual", "Rotating", "None"]],
      ["whitelist_patterns", "Only download", "Glob patterns, e.g. *.mkv; empty for all", "s?"],
      ["blacklist_patterns", "Never download", "Glob patterns, e.g. *.exe", "s?"],
      ["blocked_extensions", "Blocked extensions", "One per line, without the dot", "l"],
      ["reverse_sort", "Reverse sort", "Queue in the opposite order", "b"],
      ["on_existing", "When a file already exists", null, [
        ["resume", "Resume", "Finish a partial file, skip a complete one; a different file is kept and the new one saved as name (1)"],
        ["overwrite", "Overwrite", "Replace it with the Put.io copy once the download completes"],
        ["keep_both", "Keep both", "Keep it and save the new file as name (1), (2)…"],
      ]],
      ["resume_enabled", "Resume", "Continue a partial download from where it stopped; off starts it again", "b"],
      ["delete_after_download", "Delete from Put.io", "Remove each file from Put.io once downloaded", "b"],
      ["delete_empty_folders", "Delete empty folders", "Remove Put.io folders left empty", "b"],
    ],
  },
  {
    key: "watcher",
    title: "Torrent watcher",
    fields: [
      ["paths", "Watched folders", "One per line; .torrent files found here are uploaded", "l"],
      ["refresh_cycle", "Scan interval (s)", "1 to 127", "n", 1, 127],
      ["parent_folder_id", "Put.io folder ID", "Where transfers are saved; empty for the root", "n?", 0],
      ["archive_directory", "Archive folder", "Move uploaded .torrent files here; empty to leave them", "s?"],
      ["enabled", "Enabled", "Watch the folders (each must exist)", "b"],
      ["delete_after_upload", "Delete after upload", "Remove .torrent files once uploaded", "b"],
    ],
  },
  {
    key: "cleanup",
    title: "Put.io cleanup",
    fields: [
      ["run_at", "Daily run at", "24-hour local time, HH:MM", "s"],
      ["min_age_hours", "Minimum age (hours)", "Keep empty folders newer than this; 0 to 8760", "n", 0, 8760],
      ["enabled", "Daily run", "Remove empty Put.io folders once a day (daemon only)", "b"],
      ["nested", "Nested empty folders", "Also remove folders whose sub-folders are all empty", "b"],
    ],
  },
  {
    key: "storage",
    title: "Put.io storage",
    fields: [
      ["warn_free_percent", "Warn below (% free)", "Warn on the Put.io and Status pages and in the doctor; 1 to 89", "n", 1, 89],
      ["target_free_percent", "Free up to (% free)", "Free up space deletes old downloaded files until this is free; up to 90", "n", 2, 90],
      ["run_at", "Daily run at", "24-hour local time, HH:MM", "s"],
      ["auto_free", "Automatic free-up", "Once a day, free up space when below the warning level (daemon only)", "b"],
    ],
  },
  {
    key: "tailscale",
    title: "Tailscale",
    fields: [
      ["identity_sign_in", "Tailscale sign-in", "Let the allowed Tailscale users in without the API token", "b"],
      ["allowed_users", "Allowed users", "Tailscale logins, one per line, e.g. alice@example.com", "l"],
      ["tailnet_only", "Tailnet only", "Listen on the Tailscale address only, so the LAN can't reach the API (daemon, after a restart)", "b"],
      ["socket", "tailscaled socket", "Empty for the default; in Docker, mount the host's socket", "s?"],
    ],
    extra: tailscaleExtra,
    beforeSave: tailscaleBeforeSave,
  },
  {
    key: "network",
    title: "Network",
    fields: [
      ["api_bind", "API addresses", "One per line: ip:port, [ipv6]:port, interface:port or tailnet:port (after a restart)", "l"],
      ["metrics_bind", "Metrics addresses", "One per line, same forms; must not share an address with the API (after a restart)", "l"],
    ],
    extra: networkExtra,
    beforeSave: networkBeforeSave,
  },
  {
    key: "logging",
    title: "Logging",
    fields: [
      ["level", "Level", "How much is logged", LEVELS],
      ["console_format", "Console format", "", ["Compact", "Pretty", "Json"]],
      ["file_path", "Log file", "", "s"],
      ["module_filters", "Module filters", "One per line, e.g. putio_sync_download=debug", "l"],
      ["console_enabled", "Console", "Log to the console", "b"],
      ["file_enabled", "File", "Log to the file", "b"],
    ],
  },
  {
    key: "notifications",
    title: "Notifications",
    fields: [
      ["backend", "Backend", "How completed downloads are announced", ["None", "Webhook", "Email"]],
      ["webhook_url", "Webhook URL", "Receives a POST per completed download", "s?"],
      ["email", "Email", "For the email backend", "s?"],
      ["enabled", "Enabled", "Send notifications", "b"],
    ],
  },
];
const PRECHECK = {
  key: "precheck",
  title: "Release check",
  fields: [
    ["min_size_mb", "Minimum size (MB)", "The largest wanted file must be at least this big; 0 to 100000", "n", 0, 100000],
    ["lookup_timeout_secs", "Lookup timeout (s)", "Total time for all caches; 1 to 30", "n", 1, 30],
    ["caches", "Magnet caches", "One URL per line, with {HASH} or {hash}. Only the info hash is sent.", "l"],
    ["enabled", "Enabled", "Check releases before they reach Put.io or the NAS", "b"],
    ["dry_run", "Check only (don't reject or delete)", "Log and count what would be rejected", "b"],
    ["allow_archives", "Allow archives", "Count .rar, .7z and split parts as wanted", "b"],
    ["magnet_lookup", "Look up magnets", "Find a magnet's .torrent in the caches before adding it", "b"],
    ["hold_magnet_secs", "Hold magnets (s)", "Hold magnets no cache knows; 0 (off) to 3600", "n", 0, 3600],
    ["hold_retry_secs", "Ask again every (s)", "10 to 600", "n", 10, 600],
    ["first_lookup_secs", "First ask (s)", "While holding; 1 to the lookup timeout", "n", 1, 30],
  ],
  extra: precheckExtra,
};
const PUTIO_AUTH = { token: "OAuth token set", app_password: "app password set", none: "not set" };

let seq = 0;
let assist = null;
let riskBox = null;

/** GET /settings/assistant once per page (help, defaults, risks); null when it fails */
const assistant = () => (assist ||= settingsAssistant().catch(() => null));

/** Refetch the risks of the saved config into the banner at the top */
async function showRisks() {
  assist = null;
  const box = riskBox;
  const risks = ((await assistant()) || {}).risks || [];
  if (box !== riskBox) return;
  box.hidden = !risks.length;
  box.replaceChildren(h("h2", { text: "Warnings" }), ...riskList(risks));
}

const riskList = (risks) =>
  risks.map((r) => h("p", { class: r.level === "danger" ? "error-text" : "warn-text" }, h("strong", { text: r.level === "danger" ? "Danger: " : "Warning: " }), r.message));

const shown = (v) => {
  const s = v == null ? "empty" : Array.isArray(v) ? v.join(", ") || "none" : typeof v === "object" ? JSON.stringify(v) : String(v);
  return s.length > 60 ? `${s.slice(0, 57)}…` : s;
};

/** A preview: what would change and whether it applies now, the refusal, and the risks */
function previewNodes(p) {
  if (!p.valid) return h("p", { class: "error-text", text: `Would be refused: ${p.error}` });
  const changes = p.changes.length
    ? h(
        "ul",
        { class: "changes" },
        p.changes.map((c) => h("li", {}, h("code", { text: c.key }), ` ${shown(c.before)} → ${shown(c.after)} `, h("span", { class: `badge ${c.live ? "ok" : "warn"}`, text: c.live ? "applies now" : "needs a restart" }))),
      )
    : h("p", { class: "muted", text: "Nothing would change." });
  return h("div", {}, changes, riskList(p.risks || []));
}

export function mount(el) {
  const mine = ++seq;
  assist = null;
  riskBox = h("section", { class: "card", role: "status", hidden: true });
  const secrets = h("dl", { class: "kv" }, h("dt", { text: "Put.io" }), h("dd", { class: "muted", text: "…" }));
  el.append(
    h("div", { class: "page-head" }, h("div", {}, h("h1", { text: "Settings" }), h("p", { class: "muted", text: "Saved to the config file. Each save says whether it applies now or after a restart." }))),
    h(
      "div",
      { class: "stack" },
      riskBox,
      appearance(),
      h("section", { class: "card" }, h("h2", { text: "Credentials" }), secrets, h("p", { class: "muted spaced", text: "Tokens and passwords are never shown here. Change them in the config file or environment." })),
      arrCard(mine),
      GROUPS.map((g) => group(g, mine)),
      precheckCard(mine),
      settingsCards(() => mine === seq),
      backupCard(() => {
        unmount();
        el.replaceChildren();
        mount(el);
      }),
    ),
  );
  showRisks();
  get("/status")
    .then((s) => {
      if (mine !== seq) return;
      const auth = (s && s.auth) || {};
      secrets.replaceChildren(
        h("dt", { text: "Put.io" }),
        h("dd", { text: PUTIO_AUTH[auth.putio] || "unknown" }),
        h("dt", { text: "API token" }),
        h("dd", { text: auth.api_token ? "set" : "not set" }),
      );
    })
    .catch(() => {
      if (mine === seq) secrets.lastChild.textContent = "unknown";
    });
}

export function unmount() {
  seq += 1;
  document.removeEventListener("appearancechange", syncAppearance);
}

// ---- Appearance (feature 012) -----------------------------------------------

const MODE_NAMES = { system: "System", light: "Light", dark: "Dark" };
let appearanceForm = null;

/** Mode and accent radios; a change applies at once and is saved in this browser. */
function appearance() {
  const { MODES, ACCENTS } = window.appearance;
  const radio = (name, value, label, attrs = {}) =>
    h("label", attrs, h("input", { type: "radio", name, value, onchange: () => window.appearance.set({ [name]: value }) }), h("span", { text: label }));
  appearanceForm = h(
    "form",
    { class: "appearance-grid", onsubmit: (ev) => ev.preventDefault() },
    h("fieldset", { class: "choice" }, h("legend", { text: "Mode" }), h("div", { class: "seg-control" }, MODES.map((m) => radio("mode", m, MODE_NAMES[m])))),
    h("fieldset", { class: "choice swatches" }, h("legend", { text: "Accent" }), ACCENTS.map((a) => radio("accent", a, a[0].toUpperCase() + a.slice(1), { dataset: { accent: a } }))),
  );
  syncAppearance();
  document.addEventListener("appearancechange", syncAppearance);
  return h("section", { class: "card" }, h("h2", { text: "Appearance" }), appearanceForm, h("p", { class: "muted spaced", text: "Saved in this browser only." }));
}

function syncAppearance() {
  const now = window.appearance.get();
  for (const input of appearanceForm.querySelectorAll("input")) input.checked = now[input.name] === input.value;
}

// ---- One group --------------------------------------------------------------

function group(g, mine) {
  const card = h("section", { class: "card" });
  const load = async () => {
    card.replaceChildren(h("h2", { text: g.title }), h("p", { class: "muted", text: "Loading…" }));
    try {
      const [data, a] = await Promise.all([get(`/settings/${g.key}`), assistant()]);
      if (mine === seq) card.replaceChildren(form(g, data, load, a && a.groups[g.key]));
    } catch (e) {
      if (mine !== seq || e.status === 401 || e.status === 429) return;
      card.replaceChildren(
        h("h2", { text: g.title }),
        h("p", { class: "error-text", text: reason(e) }),
        h("button", { type: "button", class: "btn spaced", onclick: load }, "Retry"),
      );
    }
  };
  load();
  return card;
}

function form(g, data, reload, help) {
  const inputs = {};
  const note = h("p", { class: "muted", role: "status" });
  const save = h("button", { type: "submit", class: "btn btn-primary" }, "Save");
  // What the field affects and whether a change applies now (feature 015)
  const affects = (name) => {
    const text = help && help.affects[name];
    if (!text) return null;
    const live = help.live.includes(name);
    return h("small", { class: "affects" }, `Affects: ${text}. `, h("span", { class: `badge ${live ? "ok" : "warn"}`, text: live ? "applies now" : "needs a restart" }));
  };
  const build = ([name, label, hint, kind, min, max]) => {
    const id = `set-${g.key}-${name}`;
    const value = data[name];
    let input;
    if (kind === "b") {
      input = h("input", { type: "checkbox", id, checked: Boolean(value) });
      inputs[name] = () => input.checked;
      return h("label", { class: "field check", for: id }, input, h("span", {}, label, hint ? h("small", { text: hint }) : null));
    }
    if (Array.isArray(kind)) {
      // Each option is a value, or [value, label, help]; the help line follows the choice
      const opts = kind.map((o) => (Array.isArray(o) ? o : [o, o]));
      if (value != null && !opts.some(([v]) => v === value)) opts.unshift([value, value]);
      input = h("select", { class: "input", id }, opts.map(([v, text]) => h("option", { value: v, selected: v === value, text })));
      inputs[name] = () => input.value;
      const help = opts.some((o) => o[2]) ? h("small") : null;
      if (help) {
        const update = () => (help.textContent = opts.find(([v]) => v === input.value)?.[2] || "");
        input.addEventListener("change", update);
        update();
        return h("label", { class: "field", for: id }, h("span", { text: label }), input, help);
      }
    } else if (kind === "l") {
      input = h("textarea", { class: "input", id, rows: 3, spellcheck: "false" });
      input.value = (value || []).join("\n");
      inputs[name] = () => input.value.split("\n").map((s) => s.trim()).filter(Boolean);
    } else {
      const num = kind[0] === "n";
      const optional = kind.endsWith("?");
      input = h("input", { class: "input", id, type: num ? "number" : "text", min, max, step: num ? 1 : null, required: !optional, autocomplete: "off", spellcheck: "false" });
      input.value = value == null ? "" : String(value);
      inputs[name] = () => {
        const v = input.value.trim();
        if (v === "") return optional ? null : v;
        return num ? Number(v) : v;
      };
    }
    return h("label", { class: "field", for: id }, h("span", { text: label }), input, hint ? h("small", { text: hint }) : null);
  };
  const fields = g.fields.map((f) => {
    const node = build(f);
    const line = affects(f[0]);
    if (line) (f[3] === "b" ? node.lastChild : node).append(line);
    return node;
  });
  const error = h("p", { class: "error-text", role: "alert", hidden: true });
  const extra = g.extra ? g.extra(data) : null;
  const read = () => {
    const body = { ...data };
    for (const [name, value] of Object.entries(inputs)) body[name] = value();
    if (extra) Object.assign(body, extra.read());
    delete body.effective;
    return body;
  };

  // Preview the save 400 ms after the last edit; a newer edit drops an older answer
  const panel = h("div", { class: "preview", "aria-live": "polite", hidden: true });
  let timer = 0;
  let asked = 0;
  const preview = () => {
    clearTimeout(timer);
    timer = setTimeout(async () => {
      const mine = ++asked;
      try {
        const p = await previewSettings(g.key, read());
        if (mine !== asked || !panel.isConnected) return;
        panel.replaceChildren(h("h3", { text: "If you save" }), previewNodes(p));
        panel.hidden = false;
      } catch (e) {
        if (mine === asked && e.status !== 401 && e.status !== 429) {
          panel.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
          panel.hidden = false;
        }
      }
    }, 400);
  };
  const defaults =
    help && help.defaults
      ? h(
          "button",
          {
            type: "button",
            class: "btn",
            title: "Fill in the defaults; nothing is saved until Save",
            onclick: () => {
              const fresh = form(g, { ...data, ...help.defaults }, reload, help);
              el.replaceWith(fresh);
              fresh.querySelector("[role=status]").textContent = "Defaults filled in; not saved yet.";
              fresh.dispatchEvent(new Event("change"));
            },
          },
          "Defaults",
        )
      : null;

  const onSubmit = async (ev) => {
    ev.preventDefault();
    clearTimeout(timer);
    asked += 1;
    const body = read();
    if (g.beforeSave && !(await g.beforeSave(body, data))) return;
    save.disabled = true;
    error.hidden = true;
    note.textContent = "";
    try {
      const res = await put(`/settings/${g.key}`, body);
      const message = (res && res.message) || "Saved";
      toast(message);
      showRisks();
      await reload();
      // reload replaced the form; show the message on the new one
      const fresh = document.querySelector(`form[data-group="${g.key}"] [role=status]`);
      if (fresh) fresh.textContent = message;
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      error.textContent = reason(e);
      error.hidden = false;
      if (save.isConnected) save.disabled = false;
    }
  };

  const el = h(
    "form",
    {
      "data-group": g.key,
      onsubmit: onSubmit,
      oninput: () => {
        note.textContent = "";
        preview();
      },
      onchange: preview,
    },
    h("h2", { text: g.title }),
    h("div", { class: "fields" }, fields),
    extra ? extra.nodes : null,
    panel,
    error,
    h("div", { class: "form-foot" }, save, defaults, note),
  );
  return el;
}

// ---- Backup and restore (feature 015) ---------------------------------------

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
      if (e.status !== 401 && e.status !== 429) show(h("p", { class: "error-text", text: reason(e) }));
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
      if (e.status !== 401 && e.status !== 429) show(h("p", { class: "error-text", text: reason(e) }));
    }
  });
  const download = async () => {
    try {
      const url = URL.createObjectURL(new Blob([await exportSettings()], { type: "text/plain" }));
      h("a", { href: url, download: "putio-sync-settings.toml" }).click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (e) {
      if (e.status !== 401 && e.status !== 429) toast(reason(e), "error");
    }
  };
  return h(
    "section",
    { class: "card" },
    h("h2", { text: "Backup and restore" }),
    h("p", { class: "muted", text: "The file holds every setting except credentials and the Radarr / Sonarr section. Importing changes only the settings the file has; you see what changes first." }),
    h("div", { class: "form-foot" }, h("button", { type: "button", class: "btn", onclick: download }, "Export"), file),
    h("div", { class: "spaced" }, text),
    out,
    h("div", { class: "form-foot" }, apply),
  );
}

// ---- Release check (feature 042) -------------------------------------------

function precheckCard(mine) {
  return group(PRECHECK, mine);
}

const TRISTATE = [
  ["", "Default"],
  ["true", "On"],
  ["false", "Off"],
];

/** The override editors and the effective table; `read()` gives `{categories, watch}` */
function precheckExtra(data) {
  const categories = overrides("categories", "Per category", "The *arr category, e.g. sonarr", data.categories);
  const watch = overrides("watch", "Per watch folder", "A watched folder, as listed above", data.watch);
  return {
    nodes: [categories.node, watch.node, effectiveTable(data.effective || [])],
    read: () => ({ categories: categories.read(), watch: watch.read() }),
  };
}

/** Rows of key, enabled, minimum size and archives; empty fields come from the defaults */
function overrides(name, title, placeholder, map) {
  const rows = [];
  const body = h("tbody");
  const table = h("table", { class: "table override-table" }, h("thead", {}, h("tr", {}, h("th", { text: "Folder" }), h("th", { text: "Enabled" }), h("th", { text: "Min size (MB)" }), h("th", { text: "Archives" }), h("th"))), body);
  const none = h("p", { class: "muted", text: "None: each uses the settings above." });
  const sync = () => {
    table.hidden = !rows.length;
    none.hidden = rows.length > 0;
  };
  const select = (value, label) =>
    h("select", { class: "input", "aria-label": label }, TRISTATE.map(([v, t]) => h("option", { value: v, selected: String(value ?? "") === v, text: t })));
  const add = (key = "", o = {}) => {
    const r = {
      key: h("input", { class: "input", value: key, placeholder, "aria-label": "Folder", autocomplete: "off", spellcheck: "false" }),
      enabled: select(o.enabled, "Enabled"),
      min: h("input", { class: "input", type: "number", min: 0, max: 100000, step: 1, value: o.min_size_mb ?? "", placeholder: "Default", "aria-label": "Minimum size (MB)" }),
      archives: select(o.allow_archives, "Allow archives"),
    };
    const remove = () => {
      rows.splice(rows.indexOf(r), 1);
      tr.remove();
      sync();
    };
    const tr = h(
      "tr",
      {},
      h("td", { "data-label": "Folder" }, r.key),
      h("td", { "data-label": "Enabled" }, r.enabled),
      h("td", { "data-label": "Min size (MB)" }, r.min),
      h("td", { "data-label": "Archives" }, r.archives),
      h("td", { class: "actions-cell" }, h("button", { type: "button", class: "btn btn-small", "aria-label": `Remove ${key || "this row"}`, onclick: remove }, "Remove")),
    );
    rows.push(r);
    body.append(tr);
    sync();
    return r;
  };
  for (const [key, o] of Object.entries(map || {})) add(key, o);
  sync();
  const flag = (sel) => (sel.value === "" ? undefined : sel.value === "true");
  const node = h(
    "div",
    { class: "spaced", dataset: { overrides: name } },
    h("h3", { class: "sub", text: title }),
    table,
    none,
    h("button", { type: "button", class: "btn btn-small spaced", onclick: () => add().key.focus() }, "Add"),
  );
  const read = () => {
    const out = {};
    for (const r of rows) {
      const key = r.key.value.trim();
      if (!key) continue;
      const min = r.min.value.trim();
      out[key] = { enabled: flag(r.enabled), min_size_mb: min === "" ? undefined : Number(min), allow_archives: flag(r.archives) };
    }
    return out;
  };
  return { node, read };
}

/** What the check does per category and watch folder (FR-020), as saved */
function effectiveTable(rows) {
  const wanted = (r) => {
    if (r.wanted == null) return "From the upload folder's rule";
    const list = r.wanted.slice(0, 6).join(", ") + (r.wanted.length > 6 ? ", …" : "");
    return `${list} (${r.wanted_from ? `rule on ${r.wanted_from}` : "video default"})`;
  };
  return h(
    "div",
    { class: "spaced" },
    h("h3", { class: "sub", text: "What gets checked" }),
    rows.length
      ? h(
          "table",
          { class: "table" },
          h("thead", {}, h("tr", {}, h("th", { text: "Folder" }), h("th", { text: "Checked" }), h("th", { class: "right", text: "Min size" }), h("th", { text: "Archives" }), h("th", { text: "Wanted types" }))),
          h(
            "tbody",
            {},
            rows.map((r) =>
              h(
                "tr",
                {},
                h("td", { class: "name-cell name", "data-label": "Folder" }, r.key, h("small", { text: r.kind === "watch" ? " watch folder" : " category" })),
                h("td", { "data-label": "Checked", text: r.enabled ? "Yes" : "No" }),
                h("td", { class: "num right nowrap", "data-label": "Min size", text: `${r.min_size_mb} MB` }),
                h("td", { "data-label": "Archives", text: r.allow_archives ? "Allowed" : "No" }),
                h("td", { "data-label": "Wanted types", text: wanted(r) }),
              ),
            ),
          ),
        )
      : h("p", { class: "muted", text: "No categories or watch folders yet." }),
    h("p", { class: "muted spaced", text: "As saved; changes apply after a restart." }),
  );
}

// ---- Radarr / Sonarr --------------------------------------------------------

// FR-026 reasons (plus the server test's no_api_key): what happened, then what to check (US8)
const ARR_HELP = {
  unreachable: ["Unreachable", "Check the address and port, and that the app is running."],
  timed_out: ["Unreachable (timed out)", "The app didn't answer in time. Check it isn't stuck or behind a firewall."],
  untrusted_certificate: ["Untrusted HTTPS certificate", "Use the http:// address, or give the app a certificate this machine trusts."],
  unauthorized: ["API key refused", "Copy the key again from Settings → General in Radarr or Sonarr."],
  not_arr: ["Not a Radarr or Sonarr server", "Check the port: Radarr is usually 7878 and Sonarr 8989. Add the URL base if it has one."],
  unsupported_version: ["Version 3 or later is needed", "Update Radarr or Sonarr, then try again."],
  invalid_category: ["Invalid category", "Use a folder name without .. or backslashes, or leave it empty."],
  arr_cannot_reach: ["Radarr/Sonarr can't reach putio-sync", "Enter the address Radarr/Sonarr can reach putio-sync at in “putio-sync host”, then try again."],
  arr_test_failed: ["The download client test failed", "The app's own message is above. Nothing was saved."],
  arr_refused: ["Radarr/Sonarr refused the download client", "The app's own message is above. Nothing was left behind."],
  config_write_failed: ["Could not write the config file", "Check the config file isn't read-only. Nothing was changed."],
  no_api_key: ["No API key", "Edit the application and enter its API key."],
};
const arrReason = (r) => (ARR_HELP[r] || [r])[0];
const arrNext = (r) => (ARR_HELP[r] || [])[1];
const FOUND_BY = { saved: "saved", container: "container", host: "this host", browser: "browser address" };

function arrCard(mine) {
  const card = h("section", { class: "card" });
  const title = () => h("h2", { text: "Radarr / Sonarr" });
  const load = async () => {
    card.replaceChildren(title(), h("p", { class: "muted", text: "Loading…" }));
    try {
      const data = await arrStatus();
      // replaceChildren prints null as text, unlike h(), so drop the optional parts
      if (mine === seq) card.replaceChildren(title(), ...arrBody(data, load, mine).filter(Boolean));
    } catch (e) {
      if (mine !== seq || e.status === 401 || e.status === 429) return;
      card.replaceChildren(
        title(),
        h("p", { class: "error-text", text: reason(e) }),
        h("button", { type: "button", class: "btn spaced", onclick: load }, "Retry"),
      );
    }
  };
  load();
  return card;
}

const kv = (pairs) => h("dl", { class: "kv" }, pairs.flatMap(([k, v]) => [h("dt", { text: k }), h("dd", {}, v)]));

function arrBody(data, reload, mine) {
  const c = data.client;
  const connection = kv([
    ["Type", "Transmission"],
    ["Host", h("span", { class: "name", text: c.connection.hosts.join(" or ") })],
    ["Port", String(c.connection.port)],
    ["Use SSL", c.connection.ssl ? "Yes" : "No"],
    ["URL base", c.connection.url_base],
    ["Username", c.username],
    ["Download folder", h("span", { class: "name", text: c.download_dir })],
  ]);
  return [
    c.available ? null : h("p", { class: "warn-text", text: "The download client needs the database, which isn't open." }),
    h("h3", { class: "sub", text: "Download client" }),
    clientForm(c, reload),
    h("h3", { class: "sub", text: "Connection" }),
    h("p", { class: "muted", text: "Set up automatically below, or add a Transmission download client in Radarr or Sonarr with these details. Use the first host that works." }),
    connection,
    h(
      "p",
      { class: "muted" },
      "Radarr and Sonarr import from the download folder, so they must see it: at the same path, or through a Remote Path Mapping. ",
      h("a", { href: PATH_HELP, target: "_blank", rel: "noopener", text: "How to map paths" }),
    ),
    h("h3", { class: "sub", text: "Set up automatically" }),
    setupView(reload, mine),
    h("h3", { class: "sub", text: "Applications" }),
    applications(data.applications, reload),
  ];
}

/** 32 characters from [A-Za-z0-9], from the browser's CSPRNG */
function generatePassword() {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  const bytes = crypto.getRandomValues(new Uint8Array(64));
  let out = "";
  // Reject bytes past the last whole multiple of 62, so every character is equally likely
  for (const b of bytes) {
    if (b < 248 && out.length < 32) out += chars[b % 62];
  }
  return out.length === 32 ? out : generatePassword();
}

function clientForm(c, reload) {
  const envPassword = c.env && c.env.password;
  const enabled = h("input", { type: "checkbox", id: "arr-enabled", checked: Boolean(c.enabled) });
  const username = h("input", { class: "input", id: "arr-username", type: "text", required: true, autocomplete: "off", spellcheck: "false" });
  username.value = c.username;
  const password = h("input", {
    class: "input",
    id: "arr-password",
    type: "password",
    autocomplete: "new-password",
    disabled: envPassword,
    placeholder: envPassword ? "set by environment" : c.password_set ? "set; leave empty to keep" : "not set",
  });
  const copy = h("button", { type: "button", class: "btn", hidden: true }, "Copy");
  copy.onclick = async () => {
    try {
      await navigator.clipboard.writeText(password.value);
      toast("Password copied");
    } catch {
      password.select();
    }
  };
  const generate = h("button", { type: "button", class: "btn", disabled: envPassword }, "Generate");
  generate.onclick = () => {
    password.value = generatePassword();
    // Shown until the form is saved or left, so it can be copied
    password.type = "text";
    copy.hidden = false;
  };
  const error = h("p", { class: "error-text", role: "alert", hidden: true });
  const note = h("p", { class: "muted", role: "status" });
  const save = h("button", { type: "submit", class: "btn btn-primary" }, "Save");

  const onSubmit = async (ev) => {
    ev.preventDefault();
    save.disabled = true;
    error.hidden = true;
    note.textContent = "";
    try {
      const res = await saveArrClient({ enabled: enabled.checked, username: username.value.trim(), password: envPassword ? "" : password.value });
      toast(res.message || "Saved");
      // Shown in the Applications list once the card reloads (FR-008)
      if (res.password_changed && c.password_set) passwordOffer = true;
      await reload();
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      error.textContent = reason(e);
      error.hidden = false;
      if (save.isConnected) save.disabled = false;
    }
  };

  return h(
    "form",
    { onsubmit: onSubmit, oninput: () => (note.textContent = "") },
    h(
      "div",
      { class: "fields" },
      h("label", { class: "field check", for: "arr-enabled" }, enabled, h("span", {}, "On", h("small", { text: "Answer Radarr and Sonarr as a Transmission client" }))),
      h("label", { class: "field", for: "arr-username" }, h("span", { text: "Username" }), username),
      h(
        "label",
        { class: "field", for: "arr-password" },
        h("span", { text: "Password" }),
        password,
        h("small", { text: envPassword ? "Set by environment (PUTIO_SYNC_ARR_PASSWORD)" : "Never shown once saved" }),
      ),
    ),
    h("div", { class: "form-foot" }, generate, copy),
    error,
    h("div", { class: "form-foot" }, save, note),
  );
}

let setupSeq = 0;

/** Discovery, then preview, then setup (US1). A late answer for a closed view is dropped. */
function setupView(reload, mine) {
  const wrap = h("div", { class: "stack" });
  const open = h("button", { type: "button", class: "btn" }, "Set up automatically");
  const live = (view) => mine === seq && view === setupSeq && wrap.isConnected;

  const close = () => {
    setupSeq += 1;
    wrap.replaceChildren(open);
  };
  open.onclick = () => {
    const view = ++setupSeq;
    const found = h("div", { class: "stack" }, h("p", { class: "muted", text: "Looking for Radarr and Sonarr…" }));
    const url = h("input", { class: "input", id: "arr-setup-url", type: "text", placeholder: "http://radarr:7878", autocomplete: "off", spellcheck: "false" });
    const key = h("input", { class: "input", id: "arr-setup-key", type: "password", autocomplete: "off" });
    const category = h("input", { class: "input", id: "arr-setup-category", type: "text", placeholder: "radarr or sonarr", autocomplete: "off", spellcheck: "false" });
    const host = h("input", { class: "input", id: "arr-setup-host", type: "text", placeholder: "found automatically", autocomplete: "off", spellcheck: "false" });
    const remember = h("input", { type: "checkbox", id: "arr-setup-remember" });
    // Unticked by default: the operator may still use the blackhole (US5)
    const blackhole = h("input", { type: "checkbox", id: "arr-setup-blackhole" });
    const out = h("div", { role: "status" });
    const previewBtn = h("button", { type: "button", class: "btn" }, "Preview");
    const runBtn = h("button", { type: "submit", class: "btn btn-primary" }, "Set up");
    const cancel = h("button", { type: "button", class: "btn", onclick: close }, "Close");

    const req = () => ({
      target: { url: url.value.trim() },
      api_key: key.value.trim(),
      category: category.value.trim() || null,
      host: host.value.trim() || null,
      remember: remember.checked,
      disable_blackhole: blackhole.checked,
    });
    const busy = (on) => {
      previewBtn.disabled = on;
      runBtn.disabled = on;
    };
    const failure = (r) => {
      // The host override is the fix for this one: take the operator there
      if (r.reason === "arr_cannot_reach") {
        host.focus();
        host.select();
      }
      return h(
        "div",
        {},
        h("p", { class: "error-text", text: `${arrReason(r.reason)}${r.detail ? `: ${r.detail}` : ""}` }),
        r.hosts_tried && r.hosts_tried.length ? h("p", { class: "muted", text: `Hosts tried: ${r.hosts_tried.join(", ")}` }) : null,
        arrNext(r.reason) ? h("p", { class: "muted", text: arrNext(r.reason) }) : null,
      );
    };
    const fail = (e) => {
      if (e.status === 401 || e.status === 429) return;
      out.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
    };

    previewBtn.onclick = async () => {
      busy(true);
      out.replaceChildren(h("p", { class: "muted", text: "Checking…" }));
      try {
        const plan = await previewArrSetup(req());
        if (!live(view)) return;
        if (!plan.ok) {
          out.replaceChildren(failure(plan));
          return;
        }
        if (!category.value.trim()) category.placeholder = plan.category;
        const what = plan.action === "update" ? `Update ${plan.client_name}` : plan.action === "unchanged" ? "Already set up" : `Create ${plan.client_name}`;
        const notes = [];
        if (plan.download_client.will_enable) notes.push("The download client will be turned on.");
        if (plan.download_client.will_generate_password) notes.push("A password will be generated for it.");
        out.replaceChildren(
          kv([
            ["App", `${plan.name} ${plan.version}`],
            ["Address", h("span", { class: "name", text: plan.url })],
            ["Change", what],
            ["Category", plan.category],
          ]),
          existingClients(plan.existing_clients),
          plan.blackhole_enabled
            ? h("p", { class: "warn-text", text: "An enabled Torrent Blackhole may still take releases. Tick “Disable Torrent Blackhole” to turn it off." })
            : null,
          notes.length ? h("p", { class: "muted", text: notes.join(" ") }) : null,
        );
      } catch (e) {
        if (live(view)) fail(e);
      } finally {
        if (live(view)) busy(false);
      }
    };

    const onSubmit = async (ev) => {
      ev.preventDefault();
      busy(true);
      out.replaceChildren(h("p", { class: "muted", text: "Setting up… this can take up to 30 s." }));
      try {
        const r = await runArrSetup(req());
        if (!live(view)) return;
        if (r.outcome === "failed") {
          out.replaceChildren(failure(r));
          busy(false);
          return;
        }
        const verb = r.outcome === "updated" ? "Updated" : r.outcome === "unchanged" ? "Already set up:" : "Created";
        toast(`${verb} ${r.client_name} in ${r.app} ${r.version}, host ${r.host}`);
        if (r.blackhole_disabled) toast("Torrent Blackhole disabled");
        if (r.warning) toast(r.warning);
        if (r.path_check && r.path_check.status === "not_visible") {
          toast(`${r.app} cannot see ${r.path_check.arr_path}: mount the download folder there or add a Remote Path Mapping`);
        }
        await reload();
      } catch (e) {
        if (live(view)) {
          fail(e);
          busy(false);
        }
      }
    };

    wrap.replaceChildren(
      found,
      h(
        "form",
        { onsubmit: onSubmit },
        h(
          "div",
          { class: "fields" },
          h("label", { class: "field", for: "arr-setup-url" }, h("span", { text: "Address" }), url, h("small", { text: "Pick one above, or type it. A bare host like nas is tried on ports 7878 and 8989." })),
          h("label", { class: "field", for: "arr-setup-key" }, h("span", { text: "API key" }), key, h("small", { text: "Settings → General in Radarr or Sonarr" })),
          h("label", { class: "field", for: "arr-setup-category" }, h("span", { text: "Category" }), category),
          h("label", { class: "field", for: "arr-setup-host" }, h("span", { text: "putio-sync host" }), host, h("small", { text: "Only if the automatic choice fails" })),
          h(
            "label",
            { class: "field check", for: "arr-setup-blackhole" },
            blackhole,
            h("span", {}, "Disable Torrent Blackhole", h("small", { text: "Only after putio-sync's client is saved. It's turned off, not deleted." })),
          ),
          h("label", { class: "field check", for: "arr-setup-remember" }, remember, h("span", {}, "Remember", h("small", { text: "Save this app and its API key in the config file" }))),
        ),
        h("div", { class: "form-foot" }, previewBtn, runBtn, cancel),
        out,
      ),
    );

    discoverArr()
      .then((d) => {
        if (!live(view)) return;
        // Open when nothing was found, so the operator sees where it looked (US2)
        const tried = h("details", { open: !d.instances.length }, h("summary", { class: "muted", text: `Addresses tried (${d.tried.length})` }), h("p", { class: "muted name", text: d.tried.join(", ") }));
        if (!d.instances.length) {
          found.replaceChildren(h("p", { class: "muted", text: "No Radarr or Sonarr found. Type the address below." }), tried);
          return;
        }
        const pick = (i) =>
          h("button", { type: "button", class: "btn", onclick: () => ((url.value = i.url), key.focus()) }, `${i.kind === "radarr" ? "Radarr" : "Sonarr"} at ${i.url} (${FOUND_BY[i.found_by] || i.found_by})`);
        found.replaceChildren(h("div", { class: "form-foot" }, d.instances.map(pick)), tried);
      })
      .catch((e) => {
        if (!live(view) || e.status === 401 || e.status === 429) return;
        found.replaceChildren(h("p", { class: "muted", text: `Discovery didn't run: ${reason(e)}. Type the address below.` }));
      });
  };
  wrap.append(open);
  return wrap;
}

/** The *arr's download clients, putio-sync's own marked */
function existingClients(list) {
  if (!list.length) return h("p", { class: "muted", text: "No download clients there yet." });
  return h(
    "ul",
    { class: "stack" },
    list.map((c) =>
      h(
        "li",
        {},
        h("strong", { text: c.name || "(no name)" }),
        ` ${c.implementation}`,
        c.enabled ? null : h("span", { class: "muted", text: " (off)" }),
        c.points_at_putio_sync ? [" ", h("span", { class: "badge", text: "putio-sync" })] : null,
      ),
    ),
  );
}

/** Set after a password change: the saved applications still use the old one (FR-008) */
let passwordOffer = false;

const KIND_NAME = { radarr: "Radarr", sonarr: "Sonarr" };
const DEFAULT_PORT = { radarr: 7878, sonarr: 8989 };

/** A legacy typed category as the server compares it */
const normalCategory = (c) => c.trim().replace(/^\/+|\/+$/g, "").toLowerCase();

/** The Applications list, its Add buttons and the password update offer */
function applications(list, reload) {
  const wrap = h("div", { class: "stack" });
  const rows = list.map((s) => applicationRow(s, list, reload));
  const add = (kind) => {
    const form = applicationForm(kind, null, reload, () => form.replaceWith(addBar));
    addBar.replaceWith(form);
  };
  const addBar = h(
    "div",
    { class: "form-foot" },
    h("button", { type: "button", class: "btn", onclick: () => add("radarr") }, "Add Radarr"),
    h("button", { type: "button", class: "btn", onclick: () => add("sonarr") }, "Add Sonarr"),
  );
  if (passwordOffer) wrap.append(updateOffer(list, reload));
  wrap.append(
    rows.length ? h("div", { class: "stack" }, rows) : h("p", { class: "muted", text: "No applications saved. Add one, or set one up above with Remember ticked." }),
    addBar,
  );
  return wrap;
}

/** Run setup again for each saved application with a key, so it gets the new password */
function updateOffer(list, reload) {
  const withKey = list.filter((s) => s.api_key_set);
  const out = h("div", { role: "status" });
  const update = h("button", { type: "button", class: "btn btn-primary", disabled: !withKey.length }, `Update ${withKey.length === 1 ? withKey[0].name : `${withKey.length} applications`}`);
  const dismiss = h("button", { type: "button", class: "btn" }, "Not now");
  const box = h(
    "div",
    { class: "stack" },
    h("p", { class: "warn-text", text: "Radarr and Sonarr still use the old password until their putio-sync client is updated." }),
    h("div", { class: "form-foot" }, update, dismiss),
    out,
  );
  dismiss.onclick = () => {
    passwordOffer = false;
    box.remove();
  };
  update.onclick = async () => {
    update.disabled = true;
    dismiss.disabled = true;
    const lines = [];
    for (const s of withKey) {
      out.replaceChildren(h("p", { class: "muted", text: `Updating ${s.name}…` }), ...lines);
      try {
        const r = await runArrSetup({ target: { application: s.index } });
        lines.push(
          r.outcome === "failed"
            ? h("p", { class: "error-text", text: `${s.name}: ${arrReason(r.reason)}${r.detail ? `: ${r.detail}` : ""}` })
            : h("p", {}, h("span", { class: "dot ok" }), `${s.name}: updated`),
        );
      } catch (e) {
        if (e.status === 401 || e.status === 429) return;
        lines.push(h("p", { class: "error-text", text: `${s.name}: ${reason(e)}` }));
      }
    }
    out.replaceChildren(...lines);
    passwordOffer = false;
    if (lines.every((l) => l.className !== "error-text")) {
      toast("Applications updated");
      await reload();
    } else {
      dismiss.disabled = false;
      dismiss.textContent = "Close";
    }
  };
  return box;
}

const PATH_HELP = "https://github.com/devclinton/putio-sync#path-mapping";

/** Can the application see the download folder (R10); null before a check */
function pathLines(check, app) {
  if (!check) return [];
  if (check.status === "visible") {
    return [h("p", {}, h("span", { class: "dot ok" }), `Folder visible to ${app}`)];
  }
  if (check.status === "unknown") {
    return [h("p", { class: "muted", text: `Folder not checked${check.detail ? `: ${check.detail}` : ""}` })];
  }
  const mapped = check.arr_path !== check.folder ? ` (${check.folder} after its path mappings)` : "";
  const mappings = check.mappings.length
    ? `Its path mappings: ${check.mappings.map((m) => `${m.host}: ${m.remote_path} → ${m.local_path}`).join("; ")}.`
    : "It has no path mappings.";
  return [
    h("p", { class: "error-text", text: `${app} cannot see ${check.arr_path}${mapped}` }),
    h(
      "p",
      { class: "muted" },
      `Mount putio-sync's download folder in ${app} at ${check.folder}, or add a Remote Path Mapping in ${app} from ${check.folder} to where ${app} sees it. ${mappings} `,
      h("a", { href: PATH_HELP, target: "_blank", rel: "noopener", text: "How to map paths" }),
    ),
  ];
}

/** What putio-sync knows about an application's categories (R4) */
function categoriesLine(s) {
  if (!s.api_key_set) return null;
  if (!s.read_at) {
    return h("p", { class: "muted", text: s.read_error ? `Categories not read: ${arrReason(s.read_error)}` : "Categories not read yet" });
  }
  const age = `read ${relative(s.read_at)}`;
  const stale = s.read_error ? `; last read failed: ${arrReason(s.read_error)}` : "";
  if (!s.client_present) return h("p", { class: "muted", text: `Not set up: no putio-sync client there (${age}${stale})` });
  const cats = s.categories.length ? s.categories.join(", ") : "none";
  return h("p", { class: s.read_error ? "warn-text" : "muted", text: `Category ${cats} (${age}${stale})` });
}

function testResult(r) {
  if (r.ok) {
    const cats = r.client_present ? `, category ${r.categories.join(", ") || "none"}` : ", no putio-sync client yet";
    return h("div", {}, h("p", {}, h("span", { class: "dot ok" }), `Connected to ${r.app} ${r.version}${cats}`), pathLines(r.path_check, r.app));
  }
  return h("p", { class: "error-text", text: `${arrReason(r.reason)}${r.detail ? `: ${r.detail}` : ""}. ${arrNext(r.reason) || ""}`.trim() });
}

/** One saved application: categories, warnings, Test, Set up, Edit and Delete */
function applicationRow(s, list, reload) {
  const result = h("div", { role: "status" });
  const test = h("button", { type: "button", class: "btn" }, "Test");
  // Only when the last read found no putio-sync client there
  const setUp = s.client_present === false ? h("button", { type: "button", class: "btn", disabled: !s.api_key_set }, "Set up") : null;
  const edit = h("button", { type: "button", class: "btn" }, "Edit");
  const remove = h("button", { type: "button", class: "btn" }, "Delete");
  const legacy = s.legacy_category && s.read_at && !s.categories.includes(normalCategory(s.legacy_category));

  test.onclick = async () => {
    test.disabled = true;
    result.replaceChildren(h("p", { class: "muted", text: "Testing…" }));
    try {
      const r = await testArrApplication(s.index);
      if (!row.isConnected) return;
      result.replaceChildren(testResult(r));
      // The categories were re-read: show them
      if (r.ok) await reload();
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      result.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
    } finally {
      if (test.isConnected) test.disabled = false;
    }
  };
  if (setUp) setUp.onclick = async () => {
    setUp.disabled = true;
    result.replaceChildren(h("p", { class: "muted", text: "Setting up… this can take up to 30 s." }));
    try {
      const r = await runArrSetup({ target: { application: s.index } });
      if (!row.isConnected) return;
      if (r.outcome === "failed") {
        result.replaceChildren(
          h("p", { class: "error-text", text: `${arrReason(r.reason)}${r.detail ? `: ${r.detail}` : ""}` }),
          arrNext(r.reason) ? h("p", { class: "muted", text: arrNext(r.reason) }) : null,
        );
        setUp.disabled = false;
        return;
      }
      toast(`${r.outcome === "updated" ? "Updated" : r.outcome === "unchanged" ? "Already set up:" : "Created"} ${r.client_name} in ${r.app}`);
      await reload();
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      result.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
      if (setUp.isConnected) setUp.disabled = false;
    }
  };
  edit.onclick = () => {
    row.replaceWith(applicationForm(s.kind, s, reload, () => reload()));
  };
  remove.onclick = async () => {
    const ok = await confirm({
      title: `Delete ${s.name}`,
      text: `Remove ${s.name} from putio-sync? Its putio-sync download client in ${KIND_NAME[s.kind]} stays as it is.`,
      verb: "Delete",
    });
    if (!ok) return;
    try {
      await deleteArrApplication(s.index);
      toast(`${s.name} removed`);
      await reload();
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      result.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
    }
  };

  const row = h(
    "div",
    {},
    h("div", { class: "form-foot" }, h("span", {}, h("strong", { text: s.name }), " ", h("span", { class: "badge", text: KIND_NAME[s.kind] })), test, setUp, edit, remove),
    h("p", { class: "muted name", text: `${s.url}${s.api_key_set ? "" : " (no API key)"}${s.api_key_env ? " (key from environment)" : ""}` }),
    categoriesLine(s),
    pathLines(s.path_check, s.name),
    legacy
      ? h("p", { class: "warn-text", text: `The typed category “${s.legacy_category}” is no longer used. Blocklisting follows the category of ${s.name}'s putio-sync client.` })
      : null,
    result,
  );
  return row;
}

/** Add (`s` null) or Edit form. Saving after a failed or missing test asks first (FR-012). */
function applicationForm(kind, s, reload, onCancel) {
  const id = (f) => `arr-app-${s ? s.index : "new"}-${f}`;
  const input = (f, attrs) => h("input", { class: "input", id: id(f), autocomplete: "off", spellcheck: "false", ...attrs });
  const name = input("name", { type: "text", placeholder: KIND_NAME[kind] });
  const host = input("host", { type: "text", required: true, placeholder: kind });
  const port = input("port", { type: "number", min: 1, max: 65535, placeholder: String(DEFAULT_PORT[kind]) });
  const ssl = h("input", { type: "checkbox", id: id("ssl") });
  const base = input("base", { type: "text", placeholder: "none" });
  const key = input("key", {
    type: "password",
    required: !s,
    disabled: Boolean(s && s.api_key_env),
    placeholder: s ? (s.api_key_env ? "set by environment" : s.api_key_set ? "set; leave empty to keep" : "not set") : "",
  });
  if (s) {
    name.value = s.name;
    host.value = s.host;
    port.value = s.port ? String(s.port) : "";
    ssl.checked = Boolean(s.ssl);
    base.value = s.url_base;
  }
  const out = h("div", { role: "status" });
  const error = h("p", { class: "error-text", role: "alert", hidden: true });
  const testBtn = h("button", { type: "button", class: "btn" }, "Test");
  const save = h("button", { type: "submit", class: "btn btn-primary" }, "Save");
  const cancel = h("button", { type: "button", class: "btn", onclick: onCancel }, "Cancel");
  // The last test's result for the values as they are now; any change clears it
  let passed = false;

  const body = () => ({
    kind,
    name: name.value.trim(),
    host: host.value.trim(),
    port: port.value ? Number(port.value) : null,
    ssl: ssl.checked,
    url_base: base.value.trim(),
    api_key: key.value.trim(),
  });
  const shown = (e) => {
    if (e.status === 401 || e.status === 429) return;
    error.textContent = reason(e);
    error.hidden = false;
  };

  testBtn.onclick = async () => {
    testBtn.disabled = true;
    error.hidden = true;
    out.replaceChildren(h("p", { class: "muted", text: "Testing…" }));
    try {
      const r = await testArrForm(s ? { ...body(), index: s.index } : body());
      if (!form.isConnected) return;
      passed = r.ok;
      out.replaceChildren(testResult(r));
    } catch (e) {
      out.replaceChildren();
      shown(e);
    } finally {
      if (testBtn.isConnected) testBtn.disabled = false;
    }
  };

  const onSubmit = async (ev) => {
    ev.preventDefault();
    error.hidden = true;
    if (!passed) {
      const ok = await confirm({
        title: "Save without a passing test",
        text: `${KIND_NAME[kind]} hasn't passed a test with these values. Save anyway?`,
        verb: "Save",
      });
      if (!ok) return;
    }
    save.disabled = true;
    try {
      const res = s ? await saveArrApplication(s.index, body()) : await addArrApplication(body());
      toast((res && res.message) || "Saved");
      await reload();
    } catch (e) {
      shown(e);
      if (save.isConnected) save.disabled = false;
    }
  };

  const form = h(
    "form",
    { class: "stack", onsubmit: onSubmit, oninput: () => (passed = false) },
    h("strong", { text: s ? `Edit ${s.name}` : `Add ${KIND_NAME[kind]}` }),
    h(
      "div",
      { class: "fields" },
      h("label", { class: "field", for: id("name") }, h("span", { text: "Name" }), name),
      h("label", { class: "field", for: id("host") }, h("span", { text: "Host" }), host, h("small", { text: "A name or address only, like radarr or 192.168.1.10" })),
      h("label", { class: "field", for: id("port") }, h("span", { text: "Port" }), port),
      h("label", { class: "field check", for: id("ssl") }, ssl, h("span", {}, "Use SSL")),
      h("label", { class: "field", for: id("base") }, h("span", { text: "URL base" }), base, h("small", { text: "Only behind a reverse proxy, like /radarr" })),
      h(
        "label",
        { class: "field", for: id("key") },
        h("span", { text: "API key" }),
        key,
        h("small", { text: s && s.api_key_env ? `Set by environment (PUTIO_SYNC_${kind.toUpperCase()}_API_KEY)` : "Settings → General. Never shown once saved." }),
      ),
    ),
    h("div", { class: "form-foot" }, testBtn, save, cancel),
    out,
    error,
  );
  return form;
}

function reason(e) {
  return e.details ? `${e.error}: ${e.details}` : e.error || String(e);
}
