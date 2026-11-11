// Feature 030: the Settings › Notifications page's email form and notifier rows in
// ui/notify.js. Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { MASK } from "../../ui/plugins.js";
import { EVENTS, emailBody, emailForm, emailStatusText, notifierEvents, notifierRows, notifierSummary, withEvents } from "../../ui/notify.js";

const stored = {
  enabled: true,
  host: "smtp.example.com",
  security: "starttls",
  username: "nas",
  password: MASK,
  from: "nas@example.com",
  to: ["me@example.com", "you@example.com"],
  events: ["doctor.problem", "download.finished"],
  group_minutes: 5,
};

test("an unchanged password sends the mask and a new one is sent as typed", () => {
  const form = emailForm(stored);
  assert.equal(form.to, "me@example.com, you@example.com");
  assert.equal(form.port, "");
  const body = emailBody(form);
  assert.equal(body.password, MASK);
  assert.equal(body.port, null);
  assert.deepEqual(body.to, ["me@example.com", "you@example.com"]);
  // Events go in the page's order
  assert.deepEqual(body.events, ["download.finished", "doctor.problem"]);
  form.password = { set: true, replace: true, value: "new-pass" };
  form.port = "2525";
  assert.deepEqual([emailBody(form).password, emailBody(form).port], ["new-pass", 2525]);
  // No encryption sends no login
  form.security = "none";
  assert.equal(emailBody(form).username, "");
  assert.equal(emailBody(emailForm(null)).password, "");
});

test("bad numbers go as typed so the save names the field", () => {
  const form = { ...emailForm(stored), port: "smtp", group_minutes: "soon" };
  assert.equal(emailBody(form).port, "smtp");
  assert.equal(emailBody(form).group_minutes, "soon");
});

test("the status line", () => {
  assert.equal(emailStatusText({ enabled: false, sent: 0, failed: 0, last: null }), "Off.");
  const text = emailStatusText({ enabled: true, sent: 2, failed: 1, last: { at: "2026-11-11T10:00:00Z", subject: "Downloaded", error: "smtp.example.com:587 refused: 535" } });
  assert.match(text, /^On\. 2 sent, 1 failed since the daemon started\. Last: "Downloaded" failed: smtp\.example\.com:587 refused: 535 at /);
});

const ntfy = {
  name: "ntfy",
  config_fields: [
    { key: "server", type: "url", label: "Server", default: "https://ntfy.sh" },
    { key: "topic", type: "string", label: "Topic" },
    { key: "token", type: "string", label: "Access token", secret: true },
    { key: "notify", type: "string_list", label: "Events", default: ["download.finished", "doctor.problem"] },
    { key: "group_minutes", type: "integer", label: "Group", default: 5 },
  ],
};

test("notifier rows follow the bundled order and keep other plugins", () => {
  const found = [{ name: "plex", config_fields: [] }, ntfy, { name: "pushover", error: "bad manifest" }];
  const saved = [{ name: "plex", enabled: true }, { name: "ntfy", enabled: true, config: { topic: "nas" }, secrets: { token: MASK } }];
  const rows = notifierRows(found, saved);
  assert.deepEqual(rows.map((r) => r.plugin.name), ["ntfy"]);
  const { row } = rows[0];
  assert.deepEqual(notifierEvents(row), ["download.finished", "doctor.problem"]);
  assert.equal(notifierSummary(row), "Server: https://ntfy.sh · Topic: nas · Access token: set");
  const list = withEvents(saved, row, "transfer.stuck", true);
  assert.equal(list[0].name, "plex");
  assert.deepEqual(list[1].config.notify, ["download.finished", "doctor.problem", "transfer.stuck"]);
  assert.equal(list[1].secrets.token, MASK);
  // A plugin with no saved entry is added
  assert.equal(withEvents([], notifierRows([ntfy], [])[0].row, "storage.low", true).length, 1);
});

test("every event choice is a real event name", () => {
  for (const [t] of EVENTS) assert.match(t, /^[a-z]+\.[a-z_]+$/);
});
