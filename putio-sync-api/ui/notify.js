// Settings › Notifications (feature 030): email settings in and out of the form, the
// event choices, and the bundled notifier plugins as rows. No DOM here, so
// tests/ui/notify.test.mjs runs it in node.

import { MASK, fromStatus, tick, ticked, toSettings } from "./plugins.js";

/** Event types a person would want told about, in the order the page lists them */
export const EVENTS = [
  ["download.finished", "Download finished"],
  ["download.failed", "Download failed"],
  ["transfer.added", "Transfer added"],
  ["transfer.finished", "Transfer finished"],
  ["transfer.failed", "Transfer failed"],
  ["transfer.stuck", "Transfer stuck"],
  ["sync.completed", "Sync failed"],
  ["storage.low", "Put.io storage low"],
  ["verify.failed", "Verification failed"],
  ["pipeline.step_failed", "Processing step failed"],
  ["doctor.problem", "Doctor found a problem"],
];

/** The bundled notifier plugins, in the order they are shown */
export const NOTIFIERS = ["ntfy", "discord", "telegram", "pushover"];

export const SECURITY = [
  ["starttls", "STARTTLS (587)"],
  ["tls", "TLS (465)"],
  ["none", "None (25, no login)"],
];

/** `smtp` from `GET /settings/notifications` as form values */
export function emailForm(smtp) {
  const s = smtp || {};
  return {
    enabled: s.enabled === true,
    host: s.host || "",
    port: s.port == null ? "" : String(s.port),
    security: s.security || "starttls",
    username: s.username || "",
    password: { set: Boolean(s.password), value: "", replace: false },
    from: s.from || "",
    to: (s.to || []).join(", "),
    events: [...(s.events || [])],
    group_minutes: String(s.group_minutes ?? 5),
  };
}

/** Form values as the `smtp` object the API takes; an unchanged password sends the mask */
export function emailBody(form) {
  const port = form.port.trim();
  const minutes = form.group_minutes.trim();
  return {
    enabled: form.enabled,
    host: form.host.trim(),
    // A bad number goes as typed, so the save names the field
    port: port ? (Number.isInteger(Number(port)) ? Number(port) : port) : null,
    security: form.security,
    username: form.security === "none" ? "" : form.username.trim(),
    password: form.password.replace ? form.password.value : form.password.set ? MASK : "",
    from: form.from.trim(),
    to: form.to.split(/[,;\n]/).map((s) => s.trim()).filter(Boolean),
    events: EVENTS.map(([t]) => t).filter((t) => form.events.includes(t)).concat(form.events.filter((t) => !EVENTS.some(([e]) => e === t))),
    group_minutes: minutes && Number.isInteger(Number(minutes)) ? Number(minutes) : minutes || 0,
  };
}

const when = (at) => {
  const d = new Date(at);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleString();
};

/** What the email channel has sent, in a line */
export function emailStatusText(status) {
  if (!status) return "";
  const counts = `${status.sent} sent, ${status.failed} failed since the daemon started`;
  const last = status.last;
  if (!last) return status.enabled ? `On. ${counts}.` : "Off.";
  const result = last.error ? `failed: ${last.error}` : "sent";
  return `${status.enabled ? "On" : "Off"}. ${counts}. Last: "${last.subject}" ${result} at ${when(last.at)}.`;
}

/** The notifier plugins found, in NOTIFIERS order, each with its row and saved entry */
export function notifierRows(found, stored) {
  return NOTIFIERS.map((name) => found.find((p) => p.name === name))
    .filter((p) => p && p.config_fields)
    .map((plugin) => {
      const saved = (stored || []).find((s) => s.name === plugin.name) || null;
      return { plugin, saved, row: fromStatus(plugin, saved) };
    });
}

/** The events a notifier sends */
export const notifierEvents = (row) => ticked(row, "notify");

/** The `[[plugins]]` list with one notifier's events changed and the rest as saved */
export function withEvents(stored, row, type, on) {
  tick(row, "notify", type, on);
  const entry = toSettings(row);
  const list = stored || [];
  return list.some((s) => s.name === row.name) ? list.map((s) => (s.name === row.name ? entry : s)) : [...list, entry];
}

/** A notifier's settings that are not secret, and whether each secret is set, in a line */
export function notifierSummary(row) {
  const parts = [];
  for (const f of row.fields) {
    if (f.key === "notify" || f.key === "group_minutes") continue;
    if (f.secret) parts.push(`${f.label}: ${row.secrets[f.key].set ? "set" : "not set"}`);
    else if (row.values[f.key] !== "" && row.values[f.key] !== false) parts.push(`${f.label}: ${row.values[f.key]}`);
  }
  return parts.join(" · ");
}
