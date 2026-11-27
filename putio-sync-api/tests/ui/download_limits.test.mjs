// Feature 064: download hours, the disk pause, the storage budget's preview gate and
// the forecast's wording in ui/download_limits.js, and the two new waiting reasons.
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { budgetLocked, budgetPreviewLines, describeWindow, forecastLine, runBy, windowEntry, windowRow } from "../../ui/download_limits.js";
import { settingLink, waitingLine } from "../../ui/waiting.js";

const GB = 1024 ** 3;

test("a window entry survives the editor and is trimmed", () => {
  const entry = { days: "mon-fri", start: "23:00", end: "07:00" };
  assert.deepEqual(windowEntry(windowRow(entry)), entry);
  assert.deepEqual(windowEntry({ days: " sat,sun ", start: "00:00 ", end: " 24:00" }), { days: "sat,sun", start: "00:00", end: "24:00" });
  assert.equal(describeWindow(entry), "mon-fri 23:00–07:00");
});

test("the budget can be on only for a previewed percent, or the one saved on", () => {
  const off = { budget_enabled: false, budget_used_percent: 90 };
  const on = { budget_enabled: true, budget_used_percent: 90 };
  assert.equal(budgetLocked(off, 90, null), true);
  assert.equal(budgetLocked(off, 90, 90), false);
  // Another percent than the one previewed
  assert.equal(budgetLocked(off, 80, 90), true);
  assert.equal(budgetLocked(on, 90, null), false);
  assert.equal(budgetLocked(on, 80, null), true);
  assert.equal(budgetLocked(on, 80, 80), false);
  // Out of range or not a whole number: never
  assert.equal(budgetLocked(on, 5, 5), true);
  assert.equal(budgetLocked(on, 100, 100), true);
  assert.equal(budgetLocked(on, NaN, NaN), true);
  assert.equal(budgetLocked(null, 90, null), true);
});

test("a budget preview says what would go and what could not", () => {
  const file = { id: 1, path: "/a.mkv", size: 5 * GB };
  assert.deepEqual(budgetPreviewLines({ files: [], needed_bytes: 0, total_bytes: 0, shortfall_bytes: 0 }, 90), ["Put.io is under 90% used now, so nothing would be deleted."]);
  const none = budgetPreviewLines({ files: [], needed_bytes: 5 * GB, total_bytes: 0, shortfall_bytes: 5 * GB }, 90);
  assert.equal(none.length, 1);
  assert.match(none[0], /no file is confirmed downloaded, so nothing would be deleted\.$/);
  assert.deepEqual(budgetPreviewLines({ files: [file], needed_bytes: 5 * GB, total_bytes: 5 * GB, shortfall_bytes: 0 }, 90), ["Put.io is 5.0 GB over 90% used.", "Would be deleted from Put.io now: 1 file, 5.0 GB."]);
  assert.deepEqual(budgetPreviewLines({ files: [file, file], needed_bytes: 25 * GB, total_bytes: 10 * GB, shortfall_bytes: 15 * GB }, 70), [
    "Put.io is 25.0 GB over 70% used.",
    "Would be deleted from Put.io now: 2 files, 10.0 GB.",
    "That leaves it 15.0 GB over: nothing else is confirmed downloaded.",
  ]);
});

test("a run says who started it", () => {
  assert.equal(runBy("schedule"), "automatic");
  assert.equal(runBy("budget"), "storage budget");
  assert.equal(runBy("manual"), "manual");
});

test("the forecast gives a date only when there is one", () => {
  assert.equal(forecastLine(null), null);
  assert.equal(forecastLine({ state: "from_a_newer_daemon" }), null);

  const none = forecastLine({ state: "not_enough_data", total_bytes: null, used_bytes: null, samples: 0, span_hours: 0 });
  assert.equal(none.use, null);
  assert.equal(none.text, "Not enough data yet: no reading has been taken.");
  const few = forecastLine({ state: "not_enough_data", total_bytes: 100 * GB, used_bytes: 40 * GB, samples: 5, span_hours: 4 });
  assert.equal(few.use, "40.0 GB used of 100.0 GB");
  assert.match(few.text, /there are 4 h so far\.$/);
  assert.doesNotMatch(few.text, /Full in/);

  const level = forecastLine({ state: "not_growing", total_bytes: 100 * GB, used_bytes: 40 * GB, rate_bytes_per_day: -5, samples: 100, span_hours: 99 });
  assert.match(level.text, /^Not filling up/);
  assert.equal(level.tone, "muted");

  const far = forecastLine({ state: "filling", total_bytes: 100 * GB, used_bytes: 40 * GB, rate_bytes_per_day: GB, days_left: 60, full_at: "2027-01-26T00:00:00Z", samples: 49, span_hours: 48 });
  assert.match(far.text, /^Full in about 60 days, around .*2027.*\(1\.0 GB a day\)\.$/);
  assert.equal(far.tone, "muted");
  const near = { state: "filling", total_bytes: 100 * GB, used_bytes: 99 * GB, rate_bytes_per_day: GB, days_left: 1.2, full_at: "2026-11-28T05:00:00Z", samples: 49, span_hours: 48 };
  const soon = forecastLine(near);
  assert.match(soon.text, /^Full in about 1 day, /);
  assert.equal(soon.tone, "warn");
  assert.match(forecastLine({ ...near, days_left: 0.4 }).text, /^Full in less than a day/);
  assert.deepEqual(forecastLine({ state: "full", total_bytes: GB, used_bytes: GB }), { use: "1.0 GB used of 1.0 GB", text: "Full now.", tone: "warn" });
});

test("outside the download hours says when they reopen and links to them", () => {
  const line = waitingLine({ code: "download_window", setting: "download.windows", until_local: "2026-11-27T23:00" });
  assert.equal(line.text, "Outside the download hours; downloads start again at 2026-11-27 23:00 (the daemon's clock).");
  assert.equal(line.tone, "warn");
  assert.deepEqual(line.link, { label: "Download hours", href: "#/settings/downloads?field=card-hours" });
  assert.equal(waitingLine({ code: "download_window", setting: "download.windows" }).text, "Outside the download hours.");
});

test("the disk pause gives both levels and links to the pause level", () => {
  const line = waitingLine({ code: "disk_pause", setting: "download.pause_free_space_mb", free_bytes: 3 * GB, min_free_bytes: 5 * GB, resume_bytes: 6 * GB });
  assert.equal(line.text, "Paused for disk space: 3.0 GB free, under the 5.0 GB pause level. Downloads resume at 6.0 GB free.");
  assert.deepEqual(settingLink("download.pause_free_space_mb"), { label: "Pause below free space (MB)", href: "#/settings/downloads?field=set-download-pause_free_space_mb" });
});
