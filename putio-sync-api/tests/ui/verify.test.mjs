// Feature 034: labels and summaries in ui/verify.js.
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { controls, methodLabel, progressPercent, resultLabel, stateLabel, summaryLine } from "../../ui/verify.js";

test("methods read as people say them", () => {
  assert.equal(methodLabel("crc32", 0), "CRC32");
  assert.equal(methodLabel("size", null), "Size");
  assert.equal(methodLabel("none", 0), "Not checked");
  assert.equal(methodLabel(null, null), "—");
  assert.equal(methodLabel("crc32", 2), "CRC32 · re-downloaded 2×");
});

test("results and states have labels", () => {
  assert.equal(resultLabel("mismatch"), "Does not match Put.io");
  assert.equal(resultLabel("not_on_putio"), "Not on Put.io");
  assert.equal(resultLabel("other"), "other");
  assert.equal(stateLabel("waiting"), "Waiting");
});

test("progress goes by bytes, else files", () => {
  assert.equal(progressPercent({ bytes_total: 200, bytes_done: 50, files_total: 4, files_done: 3 }), 25);
  assert.equal(progressPercent({ bytes_total: 0, bytes_done: 0, files_total: 4, files_done: 1 }), 25);
  assert.equal(progressPercent({ bytes_total: 0, files_total: 0 }), 0);
  assert.equal(progressPercent(null), 0);
});

test("summaries count what was found", () => {
  assert.equal(summaryLine(null), "No verify run yet.");
  const run = { files_total: 1200, files_done: 310, verified: 305, mismatch: 1, missing: 3, busy: 0 };
  assert.equal(summaryLine(run), "310 of 1200 files: 305 verified, 1 mismatched, 3 missing");
  assert.equal(summaryLine({ files_total: 1, files_done: 1, counts: { verified: 1 } }), "1 file: 1 verified");
  assert.equal(summaryLine({ files_total: 0, files_done: 0, counts: {} }), "0 files");
});

test("controls follow the state", () => {
  assert.deepEqual(controls("idle"), { start: true, pause: false, resume: false, stop: false });
  assert.deepEqual(controls("running"), { start: false, pause: true, resume: false, stop: true });
  assert.deepEqual(controls("paused"), { start: false, pause: false, resume: true, stop: true });
  assert.deepEqual(controls("stopping"), { start: false, pause: false, resume: false, stop: false });
});
