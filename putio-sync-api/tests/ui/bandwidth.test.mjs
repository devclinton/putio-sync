// Feature 032: limits for people and the schedule editor's rows in ui/bandwidth.js.
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { describeEntry, describeSource, entryFromRow, fromMBs, move, rate, rowFromEntry, toMBs } from "../../ui/bandwidth.js";

test("limits read as people say them", () => {
  assert.equal(rate("unlimited"), "Unlimited");
  assert.equal(rate(null), "Unlimited");
  assert.equal(rate("paused"), "Paused");
  assert.equal(rate(0), "Paused");
  assert.equal(rate(5242880), "5.0 MB/s");
  assert.equal(rate(524288), "512 KB/s");
  assert.equal(rate(100), "100 B/s");
});

test("sources name the entry", () => {
  assert.equal(describeSource("schedule", 2), "Schedule entry 2");
  assert.equal(describeSource("default", null), "Default limit");
  assert.equal(describeSource("pause"), "Paused from the Status page");
});

test("entries read as one line", () => {
  assert.equal(describeEntry({ days: "mon-fri", start: "18:00", end: "23:00", limit: "paused" }), "mon-fri 18:00–23:00 Paused");
  assert.equal(describeEntry({ days: "daily", start: "01:00", end: "07:00", limit: 2097152 }), "daily 01:00–07:00 2.0 MB/s");
});

test("MB/s to bytes and back", () => {
  assert.equal(toMBs(5242880), "5");
  assert.equal(toMBs(1572864), "1.5");
  assert.equal(fromMBs("5"), 5242880);
  assert.equal(fromMBs("1,5"), 1572864);
  assert.equal(fromMBs("0.0000001"), 1);
  for (const bad of ["", "0", "-2", "fast"]) assert.equal(fromMBs(bad), null, bad);
});

test("rows round-trip entries", () => {
  for (const entry of [
    { days: "daily", start: "01:00", end: "07:00", limit: "unlimited" },
    { days: "mon-fri", start: "18:00", end: "23:00", limit: "paused" },
    { days: "fri,sat", start: "22:00", end: "02:00", limit: 2097152 },
  ]) {
    assert.deepEqual(entryFromRow(rowFromEntry(entry)), entry);
  }
  assert.deepEqual(rowFromEntry({ days: "daily", start: "01:00", end: "07:00", limit: 1048576 }).mbs, "1");
});

test("a bad rate is sent as typed so the save names it", () => {
  const row = { days: " daily ", start: "01:00", end: "07:00", kind: "rate", mbs: "fast" };
  assert.deepEqual(entryFromRow(row), { days: "daily", start: "01:00", end: "07:00", limit: "fast" });
});

test("move swaps neighbours and ignores the edges", () => {
  assert.deepEqual(move([1, 2, 3], 0, 1), [2, 1, 3]);
  assert.deepEqual(move([1, 2, 3], 2, -1), [1, 3, 2]);
  assert.deepEqual(move([1, 2, 3], 0, -1), [1, 2, 3]);
  assert.deepEqual(move([1, 2, 3], 2, 1), [1, 2, 3]);
});
