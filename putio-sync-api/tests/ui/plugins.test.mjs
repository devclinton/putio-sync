// Features 028, 031: the Settings plugin editor's rows, hosts, labels, path rules and
// settings actions in ui/plugins.js.
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  FLOW_LIMIT_MS, MASK, applyAnswer, choiceFields, fromStatus, hostsFor, nextWait, overrides,
  parsePathMap, pathMapText, runLabel, stateLabel, tick, ticked, toSettings,
} from "../../ui/plugins.js";

const ntfy = {
  name: "ntfy",
  permissions: { http: { hosts: [], host_from_config: ["server"] }, store: false },
  config_fields: [
    { key: "server", type: "url", default: "https://ntfy.sh", required: true, secret: false },
    { key: "topic", type: "string", default: null, required: true, secret: false },
    { key: "token", type: "string", default: null, required: false, secret: true },
    { key: "priority", type: "integer", default: 3, required: false, secret: false },
    { key: "notify", type: "string_list", default: ["download.finished", "storage.low"], required: false, secret: false },
    { key: "loud", type: "boolean", default: null, required: false, secret: false },
  ],
};

test("states and runs read as people say them", () => {
  assert.equal(stateLabel("needs_approval"), "Needs approval");
  assert.equal(stateLabel("missing_config"), "Needs settings");
  assert.equal(stateLabel("disabled"), "Off");
  assert.equal(runLabel(null), "Not run yet");
  assert.equal(runLabel({ result: "success", duration_ms: 40 }), "OK in 40 ms");
  assert.equal(runLabel({ result: "failure", duration_ms: 1200, error: "ntfy.sh: HTTP 401" }), "Failed in 1.2 s: ntfy.sh: HTTP 401");
  assert.equal(runLabel({ result: "timeout", duration_ms: 5000, error: "time limit reached (5 s)" }), "time limit reached (5 s)");
});

test("hosts follow the URL fields as typed, else their default", () => {
  assert.deepEqual(hostsFor(ntfy, { server: "" }), ["ntfy.sh"]);
  assert.deepEqual(hostsFor(ntfy, { server: " https://NTFY.example.org:8443/x " }), ["ntfy.example.org"]);
  assert.deepEqual(hostsFor(ntfy, { server: "not a url" }), []);
  const both = { ...ntfy, permissions: { http: { hosts: ["b.example", "a.example"], host_from_config: ["server"] } } };
  assert.deepEqual(hostsFor(both, { server: "https://a.example" }), ["a.example", "b.example"]);
  assert.deepEqual(hostsFor({ name: "x" }, {}), []);
});

test("a plugin with no settings starts off with its defaults", () => {
  const row = fromStatus(ntfy, null);
  assert.equal(row.enabled, false);
  assert.equal(row.values.server, "https://ntfy.sh");
  assert.equal(row.values.topic, "");
  assert.equal(row.values.notify, "download.finished\nstorage.low");
  assert.equal(row.values.loud, false);
  assert.deepEqual(row.secrets.token, { set: false, value: "" });
  // Defaults are left out of the save
  assert.deepEqual(toSettings(row), { name: "ntfy", enabled: false, timeout_secs: 10, memory_mb: 32 });
});

test("settings round trip and a kept secret sends the mask", () => {
  const settings = {
    name: "ntfy", enabled: true, timeout_secs: 10, memory_mb: 32,
    approved_hosts: ["ntfy.example.org"],
    config: { server: "https://ntfy.example.org", topic: "nas", priority: 4, notify: ["storage.low"], loud: true },
    secrets: { token: MASK },
  };
  const row = fromStatus(ntfy, settings);
  assert.deepEqual(row.secrets.token, { set: true, value: "" });
  assert.deepEqual(toSettings(row), settings);
});

test("secrets are replaced or cleared, and bad numbers go as typed", () => {
  const row = fromStatus(ntfy, { name: "ntfy", enabled: true, secrets: { token: MASK } });
  row.secrets.token = { set: true, replace: true, value: "tk_new" };
  row.values.priority = "4.5";
  row.values.topic = "  nas ";
  const out = toSettings(row);
  assert.deepEqual(out.secrets, { token: "tk_new" });
  assert.equal(out.config.priority, "4.5");
  assert.equal(out.config.topic, "nas");
  row.secrets.token = { set: true, replace: true, value: "" };
  assert.ok(!("secrets" in toSettings(row)));
});

const plex = {
  name: "plex",
  permissions: { http: { hosts: [], host_from_config: ["server", "plex_tv_url"] }, store: true },
  config_fields: [
    { key: "token", type: "string", default: null, required: true, secret: true, choices: null },
    { key: "server", type: "url", default: null, required: true, secret: false, choices: "load_servers" },
    { key: "libraries", type: "string_list", default: null, required: false, secret: false, choices: "load_libraries" },
    { key: "path_map", type: "path_map", default: null, required: false, secret: false, choices: null },
    { key: "plex_tv_url", type: "url", default: "https://plex.tv", required: false, secret: false, choices: null },
  ],
};

test("path rules are one `from => to` per line", () => {
  const rules = [{ from: "/downloads", to: "/data" }, { from: "/mnt/a b", to: "D:\\Media" }];
  assert.equal(pathMapText(rules), "/downloads => /data\n/mnt/a b => D:\\Media");
  assert.deepEqual(parsePathMap(" /downloads=>/data \n\n/mnt/a b  =>  D:\\Media"), rules);
  assert.equal(parsePathMap("  \n"), undefined);
  assert.deepEqual(parsePathMap("/no/arrow"), [{ from: "/no/arrow", to: "" }]);
  const row = fromStatus(plex, { name: "plex", config: { path_map: rules } });
  assert.equal(row.values.path_map, pathMapText(rules));
  assert.deepEqual(toSettings(row).config, { path_map: rules });
});

test("an action gets the typed values and its answer fills the row", () => {
  const row = fromStatus(plex, { name: "plex", config: { server: "https://old" } });
  row.values.server = " https://nas:32400 ";
  row.values.libraries = "";
  const sent = overrides(row);
  assert.equal(sent.server, "https://nas:32400");
  assert.equal(sent.libraries, null);
  assert.equal(sent.plex_tv_url, "https://plex.tv");
  assert.ok(!("token" in sent));

  applyAnswer(row, { saved: ["token", "server"], config: { server: "https://picked:32400" } });
  assert.deepEqual(row.secrets.token, { set: true, value: "" });
  assert.equal(row.values.server, "https://picked:32400");
  assert.deepEqual(toSettings(row).secrets, { token: MASK });

  assert.deepEqual(choiceFields(row, "load_libraries"), ["libraries"]);
  assert.deepEqual(choiceFields(row, "sign_in"), []);
  tick(row, "libraries", "2", true);
  tick(row, "libraries", "1", true);
  tick(row, "libraries", "2", false);
  assert.deepEqual(ticked(row, "libraries"), ["1"]);
});

test("the next step waits as asked and the flow stops after 10 minutes", () => {
  assert.equal(nextWait({ after_seconds: 2 }, 0, 0), 2000);
  assert.equal(nextWait({}, 0, 0), 2000);
  assert.equal(nextWait({ after_seconds: 99 }, 0, 0), 30000);
  assert.equal(nextWait({ after_seconds: 2 }, 0, FLOW_LIMIT_MS - 2000), 2000);
  assert.equal(nextWait({ after_seconds: 2 }, 0, FLOW_LIMIT_MS - 1000), null);
});
