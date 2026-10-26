// Settings page: appearance (this browser only, via theme.js), then the download, watcher, Put.io cleanup, logging and notification groups
// as forms built from each GET body. Save PUTs the whole group and shows the API's
// message verbatim; a 400 shows its error under the form. Tokens are shown only as set /
// not set. The Radarr / Sonarr card is read-only, with a connection test per server.
// Feature 009: Folder rules, Presets and Recently blocked cards (rules.js).

import { arrStatus, get, put, testArrServer } from "../api.js";
import { h, toast } from "../app.js";
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
      ["skip_existing", "Skip existing", "Skip files already downloaded", "b"],
      ["resume_enabled", "Resume", "Continue interrupted downloads", "b"],
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
const PUTIO_AUTH = { token: "OAuth token set", app_password: "app password set", none: "not set" };

let seq = 0;

export function mount(el) {
  const mine = ++seq;
  const secrets = h("dl", { class: "kv" }, h("dt", { text: "Put.io" }), h("dd", { class: "muted", text: "…" }));
  el.append(
    h("div", { class: "page-head" }, h("div", {}, h("h1", { text: "Settings" }), h("p", { class: "muted", text: "Saved to the config file. Each save says whether it applies now or after a restart." }))),
    h(
      "div",
      { class: "stack" },
      appearance(),
      h("section", { class: "card" }, h("h2", { text: "Credentials" }), secrets, h("p", { class: "muted spaced", text: "Tokens and passwords are never shown here. Change them in the config file or environment." })),
      arrCard(mine),
      GROUPS.map((g) => group(g, mine)),
      settingsCards(() => mine === seq),
    ),
  );
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
      const data = await get(`/settings/${g.key}`);
      if (mine === seq) card.replaceChildren(form(g, data, load));
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

function form(g, data, reload) {
  const inputs = {};
  const note = h("p", { class: "muted", role: "status" });
  const save = h("button", { type: "submit", class: "btn btn-primary" }, "Save");
  const fields = g.fields.map(([name, label, hint, kind, min, max]) => {
    const id = `set-${g.key}-${name}`;
    const value = data[name];
    let input;
    if (kind === "b") {
      input = h("input", { type: "checkbox", id, checked: Boolean(value) });
      inputs[name] = () => input.checked;
      return h("label", { class: "field check", for: id }, input, h("span", {}, label, hint ? h("small", { text: hint }) : null));
    }
    if (Array.isArray(kind)) {
      const options = kind.includes(value) || value == null ? kind : [value, ...kind];
      input = h("select", { class: "input", id }, options.map((o) => h("option", { value: o, selected: o === value, text: o })));
      inputs[name] = () => input.value;
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
  });
  const error = h("p", { class: "error-text", role: "alert", hidden: true });

  const onSubmit = async (ev) => {
    ev.preventDefault();
    const body = { ...data };
    for (const [name, read] of Object.entries(inputs)) body[name] = read();
    save.disabled = true;
    error.hidden = true;
    note.textContent = "";
    try {
      const res = await put(`/settings/${g.key}`, body);
      const message = (res && res.message) || "Saved";
      toast(message);
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

  return h(
    "form",
    { "data-group": g.key, onsubmit: onSubmit, oninput: () => (note.textContent = "") },
    h("h2", { text: g.title }),
    h("div", { class: "fields" }, fields),
    error,
    h("div", { class: "form-foot" }, save, note),
  );
}

// ---- Radarr / Sonarr --------------------------------------------------------

const ITEM_STATES = [
  ["putio", "On Put.io"],
  ["local", "Downloading"],
  ["done", "Done"],
  ["failed", "Failed"],
];
const TEST_REASONS = { unreachable: "Unreachable", unauthorized: "API key refused", not_arr: "Wrong app", no_api_key: "No API key" };

function arrCard(mine) {
  const card = h("section", { class: "card" });
  const title = () => h("h2", { text: "Radarr / Sonarr" });
  const load = async () => {
    card.replaceChildren(title(), h("p", { class: "muted", text: "Loading…" }));
    try {
      const data = await arrStatus();
      if (mine === seq) card.replaceChildren(title(), ...arrBody(data));
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

function arrBody(data) {
  const state = data.enabled
    ? h("span", {}, h("span", { class: "dot ok" }), "On")
    : h("span", {}, h("span", { class: "dot" }), "Off");
  const kv = (pairs) => h("dl", { class: "kv" }, pairs.flatMap(([k, v]) => [h("dt", { text: k }), h("dd", {}, v)]));
  const port = location.port || (location.protocol === "https:" ? "443" : "80");
  const connection = kv([
    ["Download client", state],
    ["Type", "Transmission"],
    ["Host", location.hostname],
    ["Port", port],
    ["Use SSL", location.protocol === "https:" ? "Yes" : "No"],
    ["URL base", data.url_base],
    ["Username", data.username],
    ["Password", data.password_set ? "set" : h("span", { class: "warn-text", text: "not set" })],
    ["Download folder", h("span", { class: "name", text: data.download_dir })],
  ]);

  const seen = data.clients_seen.length
    ? kv(data.clients_seen.map((c) => [c.agent, relative(c.last_contact)]))
    : h("p", { class: "muted", text: "No Radarr or Sonarr has connected since the daemon started." });

  const counts = h(
    "dl",
    { class: "counts" },
    ITEM_STATES.map(([key, label]) => h("div", {}, h("dt", { text: label }), h("dd", { text: String(data.items[key] || 0) }))),
  );

  const servers = data.servers.length
    ? h("div", { class: "stack" }, data.servers.map(arrServer))
    : h("p", { class: "muted", text: "No servers configured. Add [[arr.servers]] to the config file to blocklist failed releases." });

  const hint = data.enabled
    ? "In Radarr or Sonarr, add a Transmission download client with these details and a category."
    : "Set enabled = true and a password under [arr] in the config file, then restart.";

  return [
    h("p", { class: "muted", text: hint }),
    connection,
    h("h3", { class: "sub", text: "Items" }),
    counts,
    h("h3", { class: "sub", text: "Last contact" }),
    seen,
    h("h3", { class: "sub", text: "Servers" }),
    servers,
    h("p", { class: "muted spaced", text: "The password and API keys are never shown here. Change them in the config file or environment." }),
  ];
}

function arrServer(s) {
  const result = h("p", { class: "muted", role: "status" });
  const button = h("button", { type: "button", class: "btn" }, "Test");
  button.onclick = async () => {
    button.disabled = true;
    result.className = "muted";
    result.textContent = "Testing…";
    try {
      const r = await testArrServer(s.index);
      if (r.ok) {
        result.className = "";
        result.replaceChildren(h("span", { class: "dot ok" }), `Connected to ${r.app} ${r.version}`);
      } else {
        result.className = "error-text";
        result.textContent = `${TEST_REASONS[r.reason] || r.reason}: ${r.detail}`;
      }
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      result.className = "error-text";
      result.textContent = reason(e);
    } finally {
      if (button.isConnected) button.disabled = false;
    }
  };
  const name = s.kind === "radarr" ? "Radarr" : "Sonarr";
  return h(
    "div",
    {},
    h(
      "div",
      { class: "form-foot" },
      h("span", {}, h("strong", { text: name }), " ", h("span", { class: "badge", text: s.category || "no category" })),
      button,
    ),
    h("p", { class: "muted name", text: `${s.url}${s.api_key_set ? "" : " (no API key)"}` }),
    result,
  );
}

function reason(e) {
  return e.details ? `${e.error}: ${e.details}` : e.error || String(e);
}
