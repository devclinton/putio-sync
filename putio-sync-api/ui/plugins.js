// The Settings › Plugins editor (features 028, 031, 048, 029): rows from `GET /plugins` and
// `GET /settings/plugins` and back, states, runs, actions, path rules, tabs and Enable
// wording. No DOM here, so tests/ui/plugins.test.mjs runs it in node.

export const MASK = "********";

const STATES = {
  disabled: "Off",
  enabled: "On",
  broken: "Broken",
  needs_approval: "Needs approval",
  missing_config: "Needs settings",
};

/** A plugin state for people */
export function stateLabel(state) {
  return STATES[state] || state;
}

const seconds = (ms) => (ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`);

/** One run for people: "OK in 40 ms", "Failed in 1.2 s: ntfy.sh: HTTP 401", "time limit reached (5 s)" */
export function runLabel(run) {
  if (!run) return "Not run yet";
  if (run.result === "timeout") return run.error || "Timed out";
  if (run.result === "success") return `OK in ${seconds(run.duration_ms)}`;
  return `Failed in ${seconds(run.duration_ms)}` + (run.error ? `: ${run.error}` : "");
}

/** Path rules as the editor shows them: one `from => to` per line */
export function pathMapText(value) {
  if (!Array.isArray(value)) return value == null ? "" : String(value);
  return value.map((r) => `${r.from ?? ""} => ${r.to ?? ""}`).join("\n");
}

/** `from => to` lines as path rules; a line without `=>` goes as typed */
export function parsePathMap(text) {
  const rules = [];
  for (const line of String(text).split("\n").map((s) => s.trim()).filter(Boolean)) {
    const at = line.indexOf("=>");
    rules.push(at < 0 ? { from: line, to: "" } : { from: line.slice(0, at).trim(), to: line.slice(at + 2).trim() });
  }
  return rules.length ? rules : undefined;
}

/** A field's value as the editor shows it: text, a checkbox, or one item per line */
function toText(field, value) {
  if (field.type === "boolean") return value === true;
  if (value == null) return "";
  if (field.type === "string_list") return Array.isArray(value) ? value.join("\n") : String(value);
  if (field.type === "path_map") return pathMapText(value);
  return String(value);
}

/** The editor's text as the value `[[plugins]]` takes; `undefined` leaves the field to its default */
function fromText(field, text) {
  if (field.type === "boolean") return text === true;
  if (field.type === "string_list") {
    const items = String(text).split("\n").map((s) => s.trim()).filter(Boolean);
    return items.length ? items : undefined;
  }
  if (field.type === "path_map") return parsePathMap(text);
  const trimmed = String(text).trim();
  if (!trimmed) return undefined;
  if (field.type === "integer" || field.type === "number") {
    const n = Number(trimmed);
    // A bad number goes as typed, so the save names the field
    return Number.isFinite(n) && (field.type === "number" || Number.isInteger(n)) ? n : trimmed;
  }
  return trimmed;
}

/** The host part of an http or https URL, lowercased; null for anything else */
function urlHost(text) {
  try {
    const u = new URL(String(text).trim());
    return /^https?:$/.test(u.protocol) && u.hostname ? u.hostname.toLowerCase() : null;
  } catch {
    return null;
  }
}

/** The hosts a plugin would contact with these values (fixed hosts and URL fields), sorted */
export function hostsFor(plugin, values) {
  const http = (plugin.permissions && plugin.permissions.http) || {};
  const hosts = [...(http.hosts || [])];
  for (const key of http.host_from_config || []) {
    const field = (plugin.config_fields || []).find((f) => f.key === key);
    const text = values && values[key];
    const value = text != null && String(text).trim() ? text : field && field.default;
    const host = value != null ? urlHost(value) : null;
    if (host) hosts.push(host);
  }
  return [...new Set(hosts)].sort();
}

/** An editor row for a plugin from `GET /plugins` and its `[[plugins]]` entry, if any */
export function fromStatus(plugin, settings) {
  const s = settings || {};
  const fields = plugin.config_fields || [];
  const values = {};
  const secrets = {};
  for (const f of fields) {
    if (f.secret) {
      const stored = s.secrets && s.secrets[f.key];
      secrets[f.key] = { set: stored != null && stored !== "", value: "" };
    } else {
      values[f.key] = toText(f, s.config && f.key in s.config ? s.config[f.key] : f.default);
    }
  }
  return {
    name: plugin.name,
    enabled: s.enabled === true,
    timeout_secs: s.timeout_secs ?? 10,
    memory_mb: s.memory_mb ?? 32,
    // Sync rules (028): their own time limit, sent only for plugins that have them
    hooks: [...(plugin.hooks || [])],
    hook_timeout_ms: s.hook_timeout_ms ?? 500,
    approved_hosts: [...(s.approved_hosts || [])],
    approved_paths: [...(s.approved_paths || [])],
    fields,
    values,
    secrets,
    // Values loaded by an action, by field key
    choices: {},
  };
}

/** A row as the `[[plugins]]` entry `PUT /settings/plugins` takes; an unchanged secret sends
 *  the mask, so it is kept */
export function toSettings(row) {
  const out = { name: row.name, enabled: row.enabled, timeout_secs: Number(row.timeout_secs), memory_mb: Number(row.memory_mb) };
  if (row.hooks && row.hooks.length) out.hook_timeout_ms = Number(row.hook_timeout_ms);
  if (row.approved_hosts.length) out.approved_hosts = [...row.approved_hosts];
  if (row.approved_paths && row.approved_paths.length) out.approved_paths = [...row.approved_paths];
  const config = {};
  const secrets = {};
  for (const f of row.fields) {
    if (f.secret) {
      const s = row.secrets[f.key];
      if (s.replace) {
        if (s.value) secrets[f.key] = s.value;
      } else if (s.set) {
        secrets[f.key] = MASK;
      }
      continue;
    }
    const value = fromText(f, row.values[f.key]);
    // A value equal to the default is left out, so a later default applies
    const fallback = f.default ?? (f.type === "boolean" ? false : null);
    if (value !== undefined && JSON.stringify(value) !== JSON.stringify(fallback)) config[f.key] = value;
  }
  if (Object.keys(config).length) out.config = config;
  if (Object.keys(secrets).length) out.secrets = secrets;
  return out;
}

/** The values that are not secret, as `POST /plugins/action` takes them; empty is `null` */
export function overrides(row) {
  const config = {};
  for (const f of row.fields) {
    if (f.secret) continue;
    const value = fromText(f, row.values[f.key]);
    config[f.key] = value === undefined ? null : value;
  }
  return config;
}

/** Take what an action saved into the row: new values show, saved secrets read "set" */
export function applyAnswer(row, answer) {
  for (const f of row.fields) {
    if (f.secret) {
      if ((answer.saved || []).includes(f.key)) row.secrets[f.key] = { set: true, value: "" };
    } else if (answer.config && f.key in answer.config) {
      row.values[f.key] = toText(f, answer.config[f.key]);
    }
  }
}

/** The fields whose choices `action` loads */
export function choiceFields(row, action) {
  return row.fields.filter((f) => f.choices === action).map((f) => f.key);
}

/** The ticked items of a list field: its lines */
export function ticked(row, key) {
  return String(row.values[key] || "").split("\n").map((s) => s.trim()).filter(Boolean);
}

/** Tick or untick `value` in a list field, keeping the order of the lines */
export function tick(row, key, value, on) {
  const items = ticked(row, key).filter((v) => v !== value);
  if (on) items.push(value);
  row.values[key] = items.join("\n");
}

/** A sign-in or other flow stops asking after this long */
export const FLOW_LIMIT_MS = 10 * 60 * 1000;

/** How long to wait before the `next` step, or null when the flow has run too long */
export function nextWait(next, startedAt, now) {
  const wait = Math.min(Math.max(Number(next.after_seconds) || 2, 1), 30) * 1000;
  return now + wait - startedAt > FLOW_LIMIT_MS ? null : wait;
}

const byName = (a, b) => a.name.localeCompare(b.name);

/** Found plugins as tabs (saved as enabled) and the rest, each in name order */
export function split(found, stored) {
  const on = (p) => (stored || []).some((s) => s.name === p.name && s.enabled === true);
  return { tabs: found.filter(on).sort(byName), available: found.filter((p) => !on(p)).sort(byName) };
}

/** The first sentence or line of a description */
export function summary(text) {
  const s = String(text || "").trim();
  const line = s.split("\n")[0];
  const at = line.search(/\.\s/);
  return at < 0 ? line : line.slice(0, at + 1);
}

/** The folders a plugin may use (`paths_needed`: `r:` or `w:` each), as words */
export function foldersText(plugin) {
  const of = (k) => (plugin.paths_needed || []).filter((p) => p[0] === k).map((p) => p.slice(2)).join(", ");
  return [of("r") && `read ${of("r")}`, of("w") && `read and write ${of("w")}`].filter(Boolean).join("; ");
}

/** What Enable asks: the hosts and folders it approves, and the URL fields still empty */
export function approvalText(plugin, row) {
  const hosts = plugin.hosts_needed || hostsFor(plugin, row.values);
  const http = (plugin.permissions && plugin.permissions.http) || {};
  const unset = (http.host_from_config || [])
    .filter((key) => {
      if (row.secrets[key]) return !row.secrets[key].set;
      const field = (plugin.config_fields || []).find((f) => f.key === key);
      return !urlHost(String(row.values[key] ?? "").trim() || (field && field.default) || "");
    })
    .map((key) => (plugin.config_fields || []).find((f) => f.key === key)?.label || key);
  const parts = [];
  if (hosts.length) parts.push(`${plugin.name} will contact ${hosts.join(", ")}. Turning it on approves ${hosts.length === 1 ? "this host" : "these hosts"}.`);
  else if (!unset.length) parts.push(`${plugin.name} makes no network calls.`);
  if (unset.length) parts.push(`It will also contact the server you enter in ${unset.join(", ")}; saving that setting approves it.`);
  const folders = foldersText(plugin);
  if (folders) parts.push(`It may ${folders}; turning it on approves ${(plugin.paths_needed || []).length === 1 ? "this folder" : "these folders"}.`);
  return parts.join(" ");
}

/** The `[[plugins]]` list with one plugin turned on or off and the rest as saved */
export function withEnabled(stored, plugin, on) {
  const list = stored || [];
  const entry = list.find((s) => s.name === plugin.name) || toSettings(fromStatus(plugin));
  const next = { ...entry, enabled: on };
  return list.some((s) => s.name === plugin.name) ? list.map((s) => (s.name === plugin.name ? next : s)) : [...list, next];
}

/** An action answer as shown: while the flow runs, a code or link from an earlier step stays
 *  when a later step (a poll) leaves it out; the last step shows only what it gives */
export function carryFlow(prev, answer, busy) {
  const keep = (key) => answer[key] || (answer.done ? null : (prev && prev[key]) || null);
  return { message: answer.message, code: keep("code"), open_url: keep("open_url"), manual: !!(prev && prev.manual), busy };
}

/** A link's short label: host and path, e.g. "plex.tv/link" */
export function linkLabel(url) {
  try {
    const u = new URL(url);
    return (u.host + u.pathname).replace(/\/$/, "");
  } catch {
    return url;
  }
}

const LOCAL = /^(localhost|127\.\d+\.\d+\.\d+|10\.\d+\.\d+\.\d+|192\.168\.\d+\.\d+|172\.(1[6-9]|2\d|3[01])\.\d+\.\d+|\[::1\]|.+\.local)$/;

/** Whether a manifest link may be shown (as putio-sync-core's valid_link): https, or http to
 *  a loopback or private host, up to 300 characters */
export function linkOk(url) {
  if (typeof url !== "string" || url.length > 300 || /[\s\p{Cc}]/u.test(url)) return false;
  const m = /^(https?):\/\/([^/?#]*)/.exec(url);
  if (!m) return false;
  const authority = m[2].slice(m[2].lastIndexOf("@") + 1);
  const host = (authority.startsWith("[") ? authority.slice(0, authority.indexOf("]") + 1) : authority.split(":")[0]).toLowerCase();
  if (!host || host === "[]") return false;
  return m[1] === "https" || LOCAL.test(host);
}

const LINKS = [
  ["docs", "Docs"],
  ["homepage", "Website"],
  ["repository", "Source"],
  ["issues", "Report a problem"],
];

/** A plugin's links in the order shown, skipping unset and refused ones */
export function links(status) {
  const set = (status && status.links) || {};
  return LINKS.filter(([key]) => linkOk(set[key])).map(([key, label]) => ({ label, href: set[key] }));
}

/** "By Ada · MIT", "By Ada", "MIT", or "" */
export function byline(status) {
  const set = (status && status.links) || {};
  return [set.author ? `By ${set.author}` : "", set.license || ""].filter(Boolean).join(" · ");
}

/** Registry entries whose name, description or author holds every word of `text` (059) */
export function filterEntries(entries, text) {
  const words = (text || "").toLowerCase().split(/\s+/).filter(Boolean);
  return (entries || []).filter((e) => {
    const hay = `${e.name} ${e.description} ${e.author}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

/** What a registry entry's button says and whether it can be pressed, by its state */
export function entryAction(entry) {
  switch (entry.state) {
    case "available":
      return { label: "Install", enabled: true };
    case "update":
      return { label: `Update to ${entry.version}`, enabled: true };
    case "installed":
      return { label: "Installed", enabled: false };
    case "needs_newer":
      return { label: `Needs putio-sync ${entry.needs}`, enabled: false };
    default:
      return { label: "Name in use", enabled: false };
  }
}

/** What a registry plugin may do, for the Install confirm: events, sync rules, hosts */
export function entryReach(entry) {
  const hosts = (entry.hosts && entry.hosts.fixed) || [];
  const fromConfig = (entry.hosts && entry.hosts.from_config) || [];
  const parts = [`Runs on: ${(entry.events || []).join(", ") || "no events"}`];
  if ((entry.hooks || []).length) parts.push(`Sync rules: ${entry.hooks.join(", ")}`);
  if (hosts.length) parts.push(`Contacts: ${hosts.join(", ")}`);
  if (fromConfig.length) parts.push(`Contacts the address in: ${fromConfig.join(", ")}`);
  if (!hosts.length && !fromConfig.length) parts.push("Contacts no hosts");
  return parts;
}
