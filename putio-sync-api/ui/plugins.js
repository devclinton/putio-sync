// Lua plugins (feature 028): the rows of the Settings › Integrations › Plugins editor, from `GET /plugins`
// and `GET /settings/plugins` and back to `[[plugins]]` entries, and states and runs for
// people. Feature 031 adds settings actions (sign-in steps, loaded choices) and path rules.
// No DOM here, so tests/ui/plugins.test.mjs runs it in node.

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

/** `from => to` lines as path rules; a line without `=>` goes as typed, so the save names
 *  the field */
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

/** The hosts a plugin would contact with these editor values: its fixed hosts and the host
 *  of each URL field it names, sorted, as the save approves them */
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
    approved_hosts: [...(s.approved_hosts || [])],
    fields,
    values,
    secrets,
    // Values loaded by an action, by field key
    choices: {},
  };
}

/** An editor row as the `[[plugins]]` entry `PUT /settings/plugins` takes. A secret left as
 *  it was sends the mask, so the stored value is kept; one replaced with nothing is cleared. */
export function toSettings(row) {
  const out = { name: row.name, enabled: row.enabled, timeout_secs: Number(row.timeout_secs), memory_mb: Number(row.memory_mb) };
  if (row.approved_hosts.length) out.approved_hosts = [...row.approved_hosts];
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
    // A value equal to the default is left out, so a later default applies; an unticked
    // box with no default is the same as no value
    const fallback = f.default ?? (f.type === "boolean" ? false : null);
    if (value !== undefined && JSON.stringify(value) !== JSON.stringify(fallback)) config[f.key] = value;
  }
  if (Object.keys(config).length) out.config = config;
  if (Object.keys(secrets).length) out.secrets = secrets;
  return out;
}

/** The editor's values that are not secret, as `POST /plugins/action` takes them: what is
 *  typed counts for the call, and an empty field is `null` (no value) */
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
