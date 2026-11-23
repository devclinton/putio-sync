// Feature 058: the usage stats receiver's validator (packaging/usage-stats-worker/validate.js).
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { MAX_BODY, MAX_VALUES, readLimited, tally, validate } from "../../../packaging/usage-stats-worker/validate.js";
import worker from "../../../packaging/usage-stats-worker/worker.js";

const kv = () => {
  const m = new Map();
  return { get: async (k, t) => (m.has(k) ? (t === "json" ? JSON.parse(m.get(k)) : m.get(k)) : null), put: async (k, v) => void m.set(k, v), m };
};
const post = (body, headers = {}) => new Request("https://x/", { method: "POST", headers: { "content-type": "application/json", "content-length": String(new TextEncoder().encode(body).length), ...headers }, body });
const streamOf = (bytes) => new ReadableStream({ start(c) { c.enqueue(bytes); c.close(); } });

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

test("an oversized body is refused with and without Content-Length", async () => {
  const env = { STATS: kv() };
  const big = JSON.stringify({ pad: "a".repeat(MAX_BODY) });
  let res = await worker.fetch(post(big), env);
  assert.equal(res.status, 413);
  const stream = new Request("https://x/", { method: "POST", headers: { "content-type": "application/json" }, body: streamOf(new TextEncoder().encode(big)), duplex: "half" });
  assert.equal(stream.headers.get("content-length"), null);
  res = await worker.fetch(stream, env);
  assert.equal(res.status, 411);
  // a lying Content-Length is caught by the byte-limited reader
  res = await worker.fetch(post(big, { "content-length": "10" }), env);
  assert.equal(res.status, 413);
  assert.equal(await readLimited(new Request("https://x/", { method: "POST", body: big })), null);
  assert.equal(env.STATS.m.size, 0);
});

test("the limit counts bytes, not characters", async () => {
  const bytes = (n) => new TextEncoder().encode("é".repeat(n / 2));
  const mk = (b) => new Request("https://x/", { method: "POST", body: streamOf(b), duplex: "half" });
  assert.equal((await readLimited(mk(bytes(MAX_BODY)))).length, MAX_BODY / 2);
  assert.equal(await readLimited(mk(bytes(MAX_BODY + 2))), null);
  // 3000 characters is under 4096 as text but 6000 bytes
  const text = "é".repeat(3000);
  assert.ok(text.length < MAX_BODY);
  assert.equal((await worker.fetch(post(text), { STATS: kv() })).status, 413);
});

test("new versions past the cap fold into other", () => {
  const counts = (i) => [["version", `1.0.${i}`], ["os", "linux"]];
  let day = {};
  for (let i = 0; i < MAX_VALUES + 50; i++) day = tally(day, counts(i));
  const versions = Object.keys(day).filter((k) => k.startsWith("version:"));
  assert.equal(versions.length, MAX_VALUES + 1);
  assert.equal(day["version:other"], 50);
  assert.equal(day["os:linux"], MAX_VALUES + 50);
  day = tally(day, counts(0));
  assert.equal(day["version:1.0.0"], 2);
  assert.equal(validate({ ...good(), version: "1.2.3-rc.1" }).ok, true);
  assert.equal(validate({ ...good(), version: "1.2.3-" + "a".repeat(30) }).ok, false);
});

test("/summary is 404 unless PUBLIC_SUMMARY=1", async () => {
  const get = () => new Request("https://x/summary");
  assert.equal((await worker.fetch(get(), { STATS: kv() })).status, 404);
  assert.equal((await worker.fetch(get(), { STATS: kv(), PUBLIC_SUMMARY: "0" })).status, 404);
  assert.equal((await worker.fetch(get(), { STATS: kv(), PUBLIC_SUMMARY: "1" })).status, 200);
});
