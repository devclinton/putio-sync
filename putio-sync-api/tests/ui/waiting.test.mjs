// Feature 066: the reason line's wording and links in ui/waiting.js.
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { putioErrorLine, settingLink, skippedLine, waitingLine } from "../../ui/waiting.js";

const GB = 1024 ** 3;

test("no reason and unknown codes show nothing", () => {
  assert.equal(waitingLine(null), null);
  assert.equal(waitingLine({}), null);
  assert.equal(waitingLine({ code: "from_a_newer_daemon" }), null);
});

test("a setting key becomes a link to its field in Settings", () => {
  assert.deepEqual(settingLink("download.workers"), { label: "Workers", href: "#/settings/downloads?field=set-download-workers" });
  assert.deepEqual(settingLink("download.min_free_space_mb"), { label: "Minimum free space (MB)", href: "#/settings/downloads?field=set-download-min_free_space_mb" });
  assert.deepEqual(settingLink("download.bandwidth_schedule"), { label: "Bandwidth schedule", href: "#/settings/downloads?field=card-bandwidth" });
  assert.equal(settingLink("download.unknown"), null);
  assert.equal(settingLink(undefined), null);
});

test("disk space says how much is free and links to the floor", () => {
  const line = waitingLine({ code: "disk_space", setting: "download.min_free_space_mb", free_bytes: 2 * GB, min_free_bytes: 10 * GB });
  assert.equal(line.text, "Waiting for disk space: 2.0 GB free, 10.0 GB must stay free.");
  assert.equal(line.tone, "warn");
  assert.equal(line.link.label, "Minimum free space (MB)");
  const big = waitingLine({ code: "file_too_big", setting: "download.min_free_space_mb", free_bytes: 15 * GB, min_free_bytes: 10 * GB });
  assert.match(big.text, /^Too big for the space left: .* Smaller files go first\.$/);
  assert.equal(big.link.href, "#/settings/downloads?field=set-download-min_free_space_mb");
});

test("unreadable free space links to the folder and the troubleshooting guide", () => {
  const line = waitingLine({ code: "disk_unreadable", setting: "download.target_directory" });
  assert.equal(line.link.label, "Download folder");
  assert.equal(line.guide.slug, "guides/troubleshooting");
});

test("bandwidth reasons name what paused the downloads", () => {
  const entry = waitingLine({ code: "bandwidth_schedule", setting: "download.bandwidth_schedule", entry: 2, until_local: "2026-11-28T06:00" });
  assert.equal(entry.text, "Paused by bandwidth schedule entry 2 until 2026-11-28 06:00 (the daemon's clock).");
  assert.equal(entry.link.href, "#/settings/downloads?field=card-bandwidth");
  assert.equal(waitingLine({ code: "bandwidth_schedule", entry: 1 }).text, "Paused by bandwidth schedule entry 1.");
  const limit = waitingLine({ code: "bandwidth_limit", setting: "download.bandwidth_limit_bytes_per_sec" });
  assert.equal(limit.link.label, "Bandwidth limit (bytes/s)");
  const pause = waitingLine({ code: "bandwidth_pause", until: new Date(Date.now() + 30 * 60000).toISOString() });
  assert.equal(pause.text, "All downloads are paused; they start again in 30 min.");
  assert.deepEqual(pause.link, { label: "Status", href: "#/status" });
  assert.equal(waitingLine({ code: "bandwidth_pause" }).text, "All downloads are paused.");
});

test("the switch, a pause and a hold have no setting to link", () => {
  for (const code of ["downloads_paused", "paused", "held", "putio_queue"]) {
    const line = waitingLine({ code });
    assert.ok(line.text, code);
    assert.equal(line.link, null, code);
    assert.equal(line.guide, null, code);
  }
});

test("waiting its turn counts the workers", () => {
  assert.equal(waitingLine({ code: "waiting_turn", setting: "download.workers", workers: 1 }).text, "Waiting its turn: 1 download runs at a time.");
  const three = waitingLine({ code: "waiting_turn", setting: "download.workers", workers: 3 });
  assert.equal(three.text, "Waiting its turn: 3 downloads run at a time.");
  assert.equal(three.link.label, "Workers");
  assert.equal(three.tone, "muted");
});

test("waiting for sync says when the next one runs", () => {
  assert.equal(waitingLine({ code: "waiting_for_sync" }).text, "Done on Put.io; the next sync picks it up");
  const soon = waitingLine({ code: "waiting_for_sync", until: new Date(Date.now() + 5 * 60000).toISOString() });
  assert.equal(soon.text, "Done on Put.io; the next sync in 5 min picks it up");
});

test("a failure says what happened, what to do, and links the guide", () => {
  for (const kind of ["network", "http", "io", "disk_full", "integrity", "other", undefined]) {
    const line = waitingLine({ code: "failed", kind });
    assert.match(line.text, /^Failed/, String(kind));
    assert.match(line.text, /[Rr]etry/, String(kind));
    assert.equal(line.tone, "error");
    assert.equal(line.guide.slug, "guides/troubleshooting");
  }
  assert.equal(waitingLine({ code: "failed", kind: "io" }).link.label, "Download folder");
  assert.equal(waitingLine({ code: "failed", kind: "network" }).link, null);
  assert.equal(waitingLine({ code: "failed" }).text, "Failed. Retry it; the log says what went wrong.");
});

test("skipped files get one honest line with the rules link", () => {
  assert.equal(skippedLine(null), null);
  assert.equal(skippedLine({ skipped: 0 }), null);
  const one = skippedLine({ skipped: 1 });
  assert.equal(one.text, "Left out by a filter or folder rule, or removed from the queue: 1 file.");
  assert.equal(one.link.href, "#/settings/rules");
  assert.equal(one.guide.slug, "guides/folder-rules");
  assert.equal(skippedLine({ skipped: 4 }).text, "Left out by a filter or folder rule, or removed from the queue: 4 files.");
});

test("a Put.io error row says where to act", () => {
  assert.equal(putioErrorLine({ stage: "nas", state: "failed" }), null);
  const line = putioErrorLine({ stage: "putio", state: "error" });
  assert.match(line.text, /Transfers page/);
  assert.equal(line.guide.slug, "guides/troubleshooting");
});
