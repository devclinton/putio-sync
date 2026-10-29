// Settings page: appearance (this browser only, via theme.js), then the download, watcher, Put.io cleanup, logging and notification groups
// as forms built from each GET body. Save PUTs the whole group and shows the API's
// message verbatim; a 400 shows its error under the form. Tokens are shown only as set /
// not set. The Radarr / Sonarr card is read-only, with a connection test per server.
// Feature 009: Folder rules, Presets and Recently blocked cards (rules.js).
// Feature 042: the Release check card, with per-category and per-watch-folder overrides
// and a read-only table of what the check does for each.

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
      precheckCard(mine),
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
  });
  const error = h("p", { class: "error-text", role: "alert", hidden: true });
  const extra = g.extra ? g.extra(data) : null;

  const onSubmit = async (ev) => {
    ev.preventDefault();
    const body = { ...data };
    for (const [name, read] of Object.entries(inputs)) body[name] = read();
    if (extra) Object.assign(body, extra.read());
    delete body.effective;
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
    extra ? extra.nodes : null,
    error,
    h("div", { class: "form-foot" }, save, note),
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
