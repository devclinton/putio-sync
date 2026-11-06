// Feature 027: the Settings hook editor's rows and run labels in ui/hooks.js.
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { MASK, blankHook, eventChoices, fromSettings, moveRow, runLabel, stagePosition, targetLabel, toSettings } from "../../ui/hooks.js";

test("runs read as people say them", () => {
  assert.equal(runLabel(null), "Not run yet");
  assert.equal(runLabel({ result: "success", duration_ms: 40, http_status: 200, attempts: 1 }), "200 OK in 40 ms");
  assert.equal(runLabel({ result: "failure", duration_ms: 35200, http_status: 502, attempts: 3 }), "HTTP 502 in 35.2 s (3 tries)");
  assert.equal(runLabel({ result: "failure", duration_ms: 1200, exit_code: 3 }), "exit 3 in 1.2 s");
  assert.equal(runLabel({ result: "success", duration_ms: 12, exit_code: 0 }), "exit 0 in 12 ms");
  assert.equal(runLabel({ result: "timeout", duration_ms: 30000, error: "timed out after 30 s" }), "timed out after 30 s");
  assert.equal(runLabel({ result: "failure", duration_ms: 3, error: "program not found: /x" }), "program not found: /x in 3 ms");
});

test("targets hide what a URL may carry", () => {
  const row = { ...blankHook(), kind: "url" };
  assert.equal(targetLabel({ ...row, url: "https://u:pw@ha.local:8123/api/webhook/abc?token=x" }), "https://ha.local:8123/api/…");
  assert.equal(targetLabel({ ...row, url: "http://10.0.0.2/hook" }), "http://10.0.0.2/hook");
  assert.equal(targetLabel({ ...row, url: "https://h/hook?token=s" }), "https://h/hook…");
  assert.equal(targetLabel({ ...row, url: "nope" }), "<invalid url>");
  assert.equal(targetLabel({ ...blankHook(), program: " /config/run.sh " }), "/config/run.sh");
});

test("event choices follow All events", () => {
  const row = { ...blankHook(), events: ["storage.low"] };
  assert.deepEqual(eventChoices(row).filter((c) => c.checked).map((c) => c.value), ["storage.low"]);
  assert.ok(eventChoices({ ...row, all: true }).every((c) => c.checked));
  assert.ok(!eventChoices(row).some((c) => c.value === "hook.test"));
});

test("command hooks round trip", () => {
  const hook = { name: "on-dl", enabled: true, events: ["download.finished"], command: ["/config/on-dl.py", "--verbose", "-x"], env: { LIBRARY: MASK }, timeout_secs: 30 };
  const row = fromSettings(hook);
  assert.equal(row.kind, "command");
  assert.equal(row.args, "--verbose\n-x");
  assert.deepEqual(row.pairs, [{ key: "LIBRARY", value: MASK, masked: true }]);
  assert.deepEqual(toSettings(row), hook);
});

test("url hooks round trip, and All events is a star", () => {
  const hook = { name: "ha", enabled: false, events: ["*"], url: "https://ha.lan/api", headers: { Authorization: "Bearer x" }, timeout_secs: 10 };
  const row = fromSettings(hook);
  assert.equal(row.kind, "url");
  assert.equal(row.all, true);
  assert.deepEqual(row.events, []);
  assert.deepEqual(toSettings(row), hook);
  // An empty key is dropped; a switch to command sends env instead of headers
  row.pairs.push({ key: " ", value: "v" });
  assert.deepEqual(toSettings(row).headers, { Authorization: "Bearer x" });
  const cmd = toSettings({ ...row, kind: "command", program: "/bin/x", args: "" });
  assert.deepEqual(cmd.command, ["/bin/x"]);
  assert.ok(!("url" in cmd) && !("headers" in cmd));
});

test("a pipeline step round-trips with its stage, required flag and categories", () => {
  const step = { name: "tidy", enabled: true, events: [], command: ["/bin/tidy"], timeout_secs: 60, stage: "post_processing", required: false, categories: ["radarr", "sonarr"] };
  const row = fromSettings(step);
  assert.equal(row.stage, "post_processing");
  assert.equal(row.required, false);
  assert.equal(row.categories, "radarr, sonarr");
  const out = toSettings(row);
  assert.equal(out.stage, "post_processing");
  assert.equal(out.required, false);
  assert.deepEqual(out.categories, ["radarr", "sonarr"]);
  assert.deepEqual(out.events, []);
});

test("an event hook sends none of the step fields", () => {
  const out = toSettings(blankHook());
  assert.equal("stage" in out, false);
  assert.equal("required" in out, false);
  assert.equal("categories" in out, false);
  const row = fromSettings({ name: "n", events: ["*"], command: ["/x"] });
  assert.equal(row.stage, "");
  assert.equal(row.required, true);
});

test("steps are reordered inside the list and numbered inside their stage", () => {
  const rows = [
    { ...blankHook(1), name: "a", stage: "post_processing" },
    { ...blankHook(2), name: "b" },
    { ...blankHook(3), name: "c", stage: "post_processing" },
  ];
  assert.deepEqual(moveRow(rows, 2, -1).map((r) => r.name), ["a", "c", "b"]);
  assert.equal(moveRow(rows, 0, -1), rows);
  assert.equal(moveRow(rows, 2, 1), rows);
  assert.equal(stagePosition(rows, 0), "1 of 2");
  assert.equal(stagePosition(rows, 2), "2 of 2");
  assert.equal(stagePosition(rows, 1), "");
});
