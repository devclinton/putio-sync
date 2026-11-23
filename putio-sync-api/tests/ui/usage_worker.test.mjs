// Feature 058: the usage stats receiver's validator (packaging/usage-stats-worker/validate.js).
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { MAX_BODY, tally, validate } from "../../../packaging/usage-stats-worker/validate.js";

const good = () => ({
  schema: 1,
  install_id: "11111111-2222-4333-8444-555555555555",
  version: "0.9.1",
  build: "container",
  os: "linux",
  arch: "x86_64",
  features: {
    arr: true,
    watch_folders: false,
    pipeline_stages: { pre_download: false, pre_processing: true, post_processing: false },
    event_hooks: false,
    plugins: ["ntfy", "plex"],
    dlna: false,
    tailscale: false,
    metrics: true,
    subtitles: false,
    crash_reports: false,
  },
  sizes: { files_per_day: "1-10", library: "100GB-1TB" },
});

test("a valid payload gives counts and no free text", () => {
  const r = validate(good());
  assert.equal(r.ok, true);
  const keys = r.counts.map(([f, v]) => `${f}:${v}`);
  for (const k of ["installs:all", "version:0.9.1", "build:container", "os:linux", "feature:arr", "feature:metrics", "stage:pre_processing", "plugin:ntfy", "plugin:plex", "library:100GB-1TB"]) {
    assert.ok(keys.includes(k), k);
  }
  assert.ok(!keys.includes("feature:dlna"));
});

test("anything off the allow-list is refused", () => {
  const cases = {
    "extra top key": (p) => (p.ip = "1.2.3.4"),
    "missing key": (p) => delete p.sizes,
    "other schema": (p) => (p.schema = 2),
    "bad id": (p) => (p.install_id = "bobs-nas"),
    "id not v4": (p) => (p.install_id = "11111111-2222-1333-8444-555555555555"),
    "bad version": (p) => (p.version = "1.2.3; drop"),
    "bad build": (p) => (p.build = "bobs-box"),
    "bad os": (p) => (p.os = "BobOS"),
    "bad arch": (p) => (p.arch = "z80"),
    "user plugin": (p) => p.features.plugins.push("bobs-private-plugin"),
    "duplicate plugin": (p) => (p.features.plugins = ["ntfy", "ntfy"]),
    "flag not a boolean": (p) => (p.features.arr = "yes"),
    "extra feature": (p) => (p.features.hostname = true),
    "extra stage": (p) => (p.features.pipeline_stages.other = true),
    "bad bucket": (p) => (p.sizes.library = "12.5 TB"),
    "extra size": (p) => (p.sizes.exact_bytes = "1"),
  };
  for (const [name, change] of Object.entries(cases)) {
    const p = good();
    change(p);
    assert.equal(validate(p).ok, false, name);
  }
  for (const v of [null, [], "x", 1]) assert.equal(validate(v).ok, false);
});

test("a tally adds one to each count", () => {
  const once = tally({}, [["os", "linux"], ["installs", "all"]]);
  const twice = tally(once, [["os", "linux"]]);
  assert.deepEqual(twice, { "os:linux": 2, "installs:all": 1 });
  assert.deepEqual(once, { "os:linux": 1, "installs:all": 1 });
  assert.equal(MAX_BODY, 4096);
});
