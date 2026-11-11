// Feature 055: the plugin workbench's helpers in ui/workbench.js.
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { anchor, errorLine, lineNumbers, mode, parseJson, requestLine, runBody, runSummary, storeText } from "../../ui/workbench.js";

test("docs anchors match mdBook's heading ids", () => {
  assert.equal(anchor("download.finished"), "downloadfinished");
  assert.equal(anchor("The Run panel"), "the-run-panel");
  assert.equal(anchor("Sync rules (filter, rename, folder)"), "sync-rules-filter-rename-folder");
});

test("line numbers count every line, the last empty one too", () => {
  assert.equal(lineNumbers("a"), "1");
  assert.equal(lineNumbers("a\nb\n"), "1\n2\n3");
});

test("an error's file and line are found in Lua's forms", () => {
  assert.deepEqual(errorLine("main.lua:3: boom"), { file: "main.lua", line: 3 });
  assert.deepEqual(errorLine('runtime error: [string "main.lua"]:12: nil value'), { file: "main.lua", line: 12 });
  assert.deepEqual(errorLine("/plugins/x/plugin.toml:4: expected ="), { file: "plugin.toml", line: 4 });
  assert.equal(errorLine("time limit reached (5 s)"), null);
  assert.equal(errorLine(null), null);
});

test("Run panel values become the run request", () => {
  assert.deepEqual(runBody({ name: "x", kind: "tick", status: "", body: "" }), {
    name: "x",
    call: { kind: "tick" },
    real: false,
    reply: { status: 200, body: "" },
  });
  const event = runBody({ name: "x", kind: "event", eventType: "download.finished", json: '{"name": "a"}', real: true });
  assert.deepEqual(event, { name: "x", call: { kind: "event", event_type: "download.finished", data: { name: "a" } }, real: true });
  const hook = runBody({ name: "x", kind: "hook", hook: "rename", json: "", status: "500", body: "no" });
  assert.deepEqual(hook.call, { kind: "hook", hook: "rename", file: {} });
  assert.deepEqual(hook.reply, { status: 500, body: "no" });
  const unsaved = runBody({ name: "x", kind: "action", action: "go", json: "", unsaved: { manifest: "m", script: "s" } });
  assert.deepEqual(unsaved.call, { kind: "action", action: "go", input: null });
  assert.equal(unsaved.script, "s");
  assert.throws(() => parseJson("{", "The event data"), /The event data is not JSON/);
});

const run = (over = {}) => ({
  request: { name: "ntfy", call: { kind: "event", event_type: "download.finished" }, real: false },
  unsaved: false,
  run: { result: "success", duration_ms: 12 },
  defined: true,
  memory_bytes: 2048,
  problems: [],
  ...over,
});

test("runs read as one line", () => {
  assert.equal(runSummary(run()), "ntfy download.finished · OK · 12 ms · 2 KiB · dry run");
  assert.equal(runSummary(run({ defined: false, memory_bytes: null, unsaved: true })), "ntfy download.finished · has no such function · 12 ms · dry run · unsaved text");
  assert.equal(runSummary(run({ problems: [{ file: "main.lua", message: "x" }] })).split(" · ")[1], "did not load");
  assert.equal(runSummary(run({ run: { result: "failure", duration_ms: 3 } })).split(" · ")[1], "failed");
  assert.match(runSummary(run({ request: { name: "p", call: { kind: "hook", hook: "rename" }, real: true } })), /^p rename hook .* for real$/);
});

test("requests and the store read plainly", () => {
  assert.equal(requestLine({ method: "POST", url: "https://ntfy.sh/t", sent: false }), "POST https://ntfy.sh/t → not sent (dry run)");
  assert.equal(requestLine({ method: "GET", url: "https://a/", sent: true, status: 200, duration_ms: 40 }), "GET https://a/ → 200 in 40 ms");
  assert.equal(requestLine({ method: "GET", url: "https://b/", sent: false, error: "host not approved" }), "GET https://b/ → host not approved");
  assert.equal(storeText(null), "No store");
  assert.equal(storeText({ a: 1 }), '{\n  "a": 1\n}');
});

test("what the workbench offers for a plugin", () => {
  assert.equal(mode({ runtime: "wasm", editable: false, source: "folder" }, true), "wasm");
  assert.equal(mode({ runtime: "lua", editable: true, source: "folder" }, true), "edit");
  assert.equal(mode({ runtime: "lua", editable: false, source: "bundled" }, true), "copy");
  assert.equal(mode({ runtime: "lua", editable: false, source: "bundled" }, false), "view");
});
